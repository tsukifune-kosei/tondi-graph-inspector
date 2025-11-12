CREATE TABLE app_config
(
    id                 BOOLEAN    PRIMARY KEY DEFAULT TRUE,
    tondid_version     TEXT       NOT NULL,
    processing_version TEXT       NOT NULL,
    CONSTRAINT unique_row CHECK (id)
);