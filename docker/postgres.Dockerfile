FROM postgres:18-trixie

RUN apt-get update && apt-get install -y postgresql-common ca-certificates \
    && /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y \
    && apt-get install -y postgresql-18-hypopg
