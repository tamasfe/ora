#![allow(dead_code)]
use std::{
    marker::PhantomData,
    ops::{Bound, RangeBounds},
};

use fjall::Slice;
use uuid::Uuid;

pub struct TxPartition<K, V> {
    partition: fjall::TxPartitionHandle,
    _p: PhantomData<fn() -> (K, V)>,
}

impl<K, V> Clone for TxPartition<K, V> {
    fn clone(&self) -> Self {
        Self {
            partition: self.partition.clone(),
            _p: PhantomData,
        }
    }
}

impl<K, V> std::fmt::Debug for TxPartition<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TxPartition").finish_non_exhaustive()
    }
}

impl<K, V> TxPartition<K, V>
where
    K: Key,
    V: FjallValue,
{
    pub fn new(partition: fjall::TxPartitionHandle) -> Self {
        Self {
            partition,
            _p: PhantomData,
        }
    }

    pub fn inner(&self) -> &fjall::TxPartitionHandle {
        &self.partition
    }
}

impl<K, V> From<fjall::TxPartitionHandle> for TxPartition<K, V>
where
    K: Key,
    V: FjallValue,
{
    fn from(partition: fjall::TxPartitionHandle) -> Self {
        Self::new(partition)
    }
}

impl<K, V> TxPartition<K, V>
where
    K: Key,
    V: FjallValue,
{
    pub fn write<'a: 'tx, 'tx>(
        &'a self,
        transaction: &'tx mut fjall::WriteTransaction<'a>,
    ) -> PartitionWriteTransaction<'a, 'tx, K, V> {
        PartitionWriteTransaction {
            partition: &self.partition,
            transaction,
            _p: PhantomData,
        }
    }

    pub fn read<'a>(
        &'a self,
        transaction: &'a fjall::ReadTransaction,
    ) -> PartitionReadTransaction<'a, K, V> {
        PartitionReadTransaction {
            partition: &self.partition,
            transaction,
            _p: PhantomData,
        }
    }
}

pub struct Raw<V> {
    slice: Slice,
    _p: PhantomData<fn() -> V>,
}

impl<V> Raw<V>
where
    V: FjallValue,
{
    pub fn value(&self) -> V::View<'_> {
        V::view_from_slice(&self.slice)
    }
}

pub struct PartitionWriteTransaction<'a, 'tx, K, V> {
    partition: &'a fjall::TxPartitionHandle,
    transaction: &'tx mut fjall::WriteTransaction<'a>,
    _p: PhantomData<fn() -> (K, V)>,
}

impl<K, V> PartitionWriteTransaction<'_, '_, K, V>
where
    K: Key,
    V: FjallValue,
{
    pub fn get(&self, key: &K) -> fjall::Result<Option<Raw<V>>> {
        self.transaction
            .get(self.partition, key.as_slice())
            .map(|slice| {
                slice.map(|slice| Raw {
                    slice,
                    _p: PhantomData,
                })
            })
    }

    pub fn contains_key(&self, key: &K) -> fjall::Result<bool> {
        self.transaction
            .contains_key(self.partition, key.as_slice())
    }

    pub fn take(&mut self, key: &K) -> fjall::Result<Option<Raw<V>>> {
        self.transaction
            .take(self.partition, key.as_slice())
            .map(|slice| {
                slice.map(|slice| Raw {
                    slice,
                    _p: PhantomData,
                })
            })
    }

    pub fn remove(&mut self, key: &K) {
        self.transaction.remove(self.partition, key.as_slice());
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = fjall::Result<Raw<K>>> + '_ {
        self.transaction.keys(self.partition).map(|result| {
            result.map(|slice| Raw {
                slice,
                _p: PhantomData,
            })
        })
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = fjall::Result<(Raw<K>, Raw<V>)>> + '_ {
        self.transaction.iter(self.partition).map(|result| {
            result.map(|(key, value)| {
                (
                    Raw {
                        slice: key,
                        _p: PhantomData,
                    },
                    Raw {
                        slice: value,
                        _p: PhantomData,
                    },
                )
            })
        })
    }

    pub fn prefix(
        &self,
        prefix: &K,
    ) -> impl DoubleEndedIterator<Item = fjall::Result<(Raw<K>, Raw<V>)>> + '_ {
        self.transaction
            .prefix(self.partition, prefix.as_slice())
            .map(|result| {
                result.map(|(key, value)| {
                    (
                        Raw {
                            slice: key,
                            _p: PhantomData,
                        },
                        Raw {
                            slice: value,
                            _p: PhantomData,
                        },
                    )
                })
            })
    }

    pub fn range<R>(
        &self,
        range: R,
    ) -> impl DoubleEndedIterator<Item = fjall::Result<(Raw<K>, Raw<V>)>> + '_
    where
        R: RangeBounds<K>,
    {
        self.transaction
            .range(self.partition, SliceBounds::new(range))
            .map(|result| {
                result.map(|(key, value)| {
                    (
                        Raw {
                            slice: key,
                            _p: PhantomData,
                        },
                        Raw {
                            slice: value,
                            _p: PhantomData,
                        },
                    )
                })
            })
    }

    pub fn insert(&mut self, key: &K, value: &V) {
        self.transaction
            .insert(self.partition, key.as_slice(), value.as_slice());
    }
}

pub struct PartitionReadTransaction<'a, K, V> {
    partition: &'a fjall::TxPartitionHandle,
    transaction: &'a fjall::ReadTransaction,
    _p: PhantomData<fn() -> (K, V)>,
}

impl<K, V> PartitionReadTransaction<'_, K, V>
where
    K: Key,
    V: FjallValue,
{
    pub fn get(&self, key: &K) -> fjall::Result<Option<Raw<V>>> {
        self.transaction
            .get(self.partition, key.as_slice())
            .map(|slice| {
                slice.map(|slice| Raw {
                    slice,
                    _p: PhantomData,
                })
            })
    }

    pub fn contains_key(&self, key: &K) -> fjall::Result<bool> {
        self.transaction
            .contains_key(self.partition, key.as_slice())
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = fjall::Result<Raw<K>>> + '_ {
        self.transaction.keys(self.partition).map(|result| {
            result.map(|slice| Raw {
                slice,
                _p: PhantomData,
            })
        })
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = fjall::Result<(Raw<K>, Raw<V>)>> + '_ {
        self.transaction.iter(self.partition).map(|result| {
            result.map(|(key, value)| {
                (
                    Raw {
                        slice: key,
                        _p: PhantomData,
                    },
                    Raw {
                        slice: value,
                        _p: PhantomData,
                    },
                )
            })
        })
    }

    pub fn is_empty(&self) -> fjall::Result<bool> {
        self.transaction.is_empty(self.partition)
    }

    pub fn range<R>(
        &self,
        range: R,
    ) -> impl DoubleEndedIterator<Item = fjall::Result<(Raw<K>, Raw<V>)>> + '_
    where
        R: RangeBounds<K>,
    {
        self.transaction
            .range(self.partition, SliceBounds::new(range))
            .map(|result| {
                result.map(|(key, value)| {
                    (
                        Raw {
                            slice: key,
                            _p: PhantomData,
                        },
                        Raw {
                            slice: value,
                            _p: PhantomData,
                        },
                    )
                })
            })
    }

    pub fn prefix(
        &self,
        prefix: &K,
    ) -> impl DoubleEndedIterator<Item = fjall::Result<(Raw<K>, Raw<V>)>> + '_ {
        self.transaction
            .prefix(self.partition, prefix.as_slice())
            .map(|result| {
                result.map(|(key, value)| {
                    (
                        Raw {
                            slice: key,
                            _p: PhantomData,
                        },
                        Raw {
                            slice: value,
                            _p: PhantomData,
                        },
                    )
                })
            })
    }
}

pub trait FjallValue {
    type View<'a>;

    fn as_slice(&self) -> Slice;
    fn view_from_slice(slice: &Slice) -> Self::View<'_>;
}

pub trait Key: FjallValue {}

impl FjallValue for Uuid {
    type View<'a> = Uuid;

    fn as_slice(&self) -> Slice {
        self.as_bytes().as_ref().into()
    }

    fn view_from_slice(slice: &Slice) -> Self::View<'_> {
        Uuid::from_slice(slice.as_ref()).unwrap()
    }
}

impl Key for Uuid {}

impl FjallValue for String {
    type View<'a> = &'a str;

    fn as_slice(&self) -> Slice {
        self.as_bytes().into()
    }

    fn view_from_slice(slice: &Slice) -> Self::View<'_> {
        std::str::from_utf8(slice.as_ref()).unwrap()
    }
}

impl Key for String {}

impl FjallValue for () {
    type View<'a> = ();

    fn as_slice(&self) -> Slice {
        Slice::new(&[])
    }

    fn view_from_slice(_slice: &Slice) -> Self::View<'_> {}
}

struct SliceBounds {
    start: Bound<Slice>,
    end: Bound<Slice>,
}

impl SliceBounds {
    fn new<K>(range: impl RangeBounds<K>) -> Self
    where
        K: Key,
    {
        Self {
            start: match range.start_bound() {
                Bound::Included(key) => Bound::Included(key.as_slice()),
                Bound::Excluded(key) => Bound::Excluded(key.as_slice()),
                Bound::Unbounded => Bound::Unbounded,
            },
            end: match range.end_bound() {
                Bound::Included(key) => Bound::Included(key.as_slice()),
                Bound::Excluded(key) => Bound::Excluded(key.as_slice()),
                Bound::Unbounded => Bound::Unbounded,
            },
        }
    }
}

impl RangeBounds<Slice> for SliceBounds {
    fn start_bound(&self) -> Bound<&Slice> {
        match &self.start {
            Bound::Included(slice) => Bound::Included(slice),
            Bound::Excluded(slice) => Bound::Excluded(slice),
            Bound::Unbounded => Bound::Unbounded,
        }
    }

    fn end_bound(&self) -> Bound<&Slice> {
        match &self.end {
            Bound::Included(slice) => Bound::Included(slice),
            Bound::Excluded(slice) => Bound::Excluded(slice),
            Bound::Unbounded => Bound::Unbounded,
        }
    }
}
