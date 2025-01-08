use bytes::{BufMut, Bytes, BytesMut};
use uuid::Uuid;

use crate::typed::{FjallValue, Key};

/// A label index key that contains the label key, label value, and an associated ID of the label.
pub struct LabelIndexKey(Bytes);

impl LabelIndexKey {
    /// Creates a new label index key for insertion into the label index.
    pub fn new(key: &str, value: &str, id: Uuid) -> Self {
        let mut bytes = BytesMut::with_capacity(key.len() + value.len() + size_of::<Uuid>());
        bytes.extend_from_slice(key.as_bytes());
        bytes.put_bytes(LABEL_DELIMITER as u8, 1);
        bytes.extend_from_slice(value.as_bytes());
        bytes.put_bytes(LABEL_DELIMITER as u8, 1);
        bytes.extend_from_slice(id.as_bytes().as_slice());
        Self(bytes.into())
    }

    /// Creates a new label index key for searching the label index by key and value.
    pub fn new_prefix_key_value(key: &str, value: &str) -> Self {
        let mut bytes = BytesMut::with_capacity(key.len() + value.len() + 2);
        bytes.extend_from_slice(key.as_bytes());
        bytes.put_bytes(LABEL_DELIMITER as u8, 1);
        bytes.extend_from_slice(value.as_bytes());
        bytes.put_bytes(LABEL_DELIMITER as u8, 1);
        Self(bytes.into())
    }

    /// Creates a new label index key for searching the label index by key.
    pub fn new_prefix_key(key: &str) -> Self {
        let mut bytes = BytesMut::with_capacity(key.len() + 1);
        bytes.extend_from_slice(key.as_bytes());
        bytes.put_bytes(LABEL_DELIMITER as u8, 1);
        Self(bytes.into())
    }

    /// Returns the key part of the label index key.
    #[allow(dead_code)]
    pub fn key(&self) -> &str {
        let bytes = self.0.as_ref();
        let delimiter = bytes
            .iter()
            .position(|&b| b == LABEL_DELIMITER as u8)
            .unwrap();
        std::str::from_utf8(&bytes[..delimiter]).unwrap()
    }

    /// Returns the value part of the label index key.
    #[allow(dead_code)]
    pub fn value(&self) -> &str {
        let bytes = self.0.as_ref();
        let start = bytes
            .iter()
            .position(|&b| b == LABEL_DELIMITER as u8)
            .unwrap()
            + 1;
        let end = bytes[start..]
            .iter()
            .position(|&b| b == LABEL_DELIMITER as u8)
            .unwrap();
        std::str::from_utf8(&bytes[start..start + end]).unwrap()
    }

    /// Returns the ID part of the label index key.
    pub fn id(&self) -> Uuid {
        // We know that the last 16 bytes are the UUID.
        let bytes = self.0.as_ref();
        let start = bytes.len() - size_of::<Uuid>();
        Uuid::from_slice(&bytes[start..]).unwrap()
    }
}

const LABEL_DELIMITER: char = '\u{001F}';

impl FjallValue for LabelIndexKey {
    type View<'a> = Self;

    fn as_slice(&self) -> fjall::Slice {
        self.0.clone().into()
    }

    fn view_from_slice(slice: &fjall::Slice) -> Self::View<'_> {
        Self(slice.clone().into())
    }
}

impl Key for LabelIndexKey {}

pub struct JobExecutionIndexKey {
    pub job_id: Uuid,
    pub execution_id: Option<Uuid>,
}

impl JobExecutionIndexKey {
    pub fn new(job_id: Uuid, execution_id: Uuid) -> Self {
        Self {
            job_id,
            execution_id: Some(execution_id),
        }
    }

    pub fn new_prefix(job_id: Uuid) -> Self {
        Self {
            job_id,
            execution_id: None,
        }
    }
}

impl FjallValue for JobExecutionIndexKey {
    type View<'a> = Self;

    fn as_slice(&self) -> fjall::Slice {
        let mut bytes = BytesMut::with_capacity(32);

        bytes.extend_from_slice(self.job_id.as_bytes());

        if let Some(execution_id) = self.execution_id {
            bytes.extend_from_slice(execution_id.as_bytes());
        }

        bytes.freeze().into()
    }

    fn view_from_slice(slice: &fjall::Slice) -> Self::View<'_> {
        let bytes = slice.as_ref();

        let job_id = Uuid::from_slice(&bytes[0..16]).unwrap();
        let execution_id = Uuid::from_slice(&bytes[16..32]).unwrap();

        Self {
            job_id,
            execution_id: Some(execution_id),
        }
    }
}

impl Key for JobExecutionIndexKey {}

pub struct ScheduleJobIndexKey {
    pub schedule_id: Uuid,
    pub job_id: Option<Uuid>,
}

impl ScheduleJobIndexKey {
    pub fn new(schedule_id: Uuid, job_id: Uuid) -> Self {
        Self {
            schedule_id,
            job_id: Some(job_id),
        }
    }

    pub fn new_prefix(schedule_id: Uuid) -> Self {
        Self {
            schedule_id,
            job_id: None,
        }
    }
}

impl FjallValue for ScheduleJobIndexKey {
    type View<'a> = Self;

    fn as_slice(&self) -> fjall::Slice {
        let mut bytes = BytesMut::with_capacity(32);

        bytes.extend_from_slice(self.schedule_id.as_bytes());

        if let Some(job_id) = self.job_id {
            bytes.extend_from_slice(job_id.as_bytes());
        }

        bytes.freeze().into()
    }

    fn view_from_slice(slice: &fjall::Slice) -> Self::View<'_> {
        let bytes = slice.as_ref();

        let schedule_id = Uuid::from_slice(&bytes[0..16]).unwrap();
        let job_id = Uuid::from_slice(&bytes[16..32]).unwrap();

        Self {
            schedule_id,
            job_id: Some(job_id),
        }
    }
}

impl Key for ScheduleJobIndexKey {}
