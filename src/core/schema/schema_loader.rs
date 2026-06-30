use anyhow::Result;

use crate::core::schema::{Column, DatabaseSchema, Table};

pub struct SchemaLoader;

impl SchemaLoader {
    pub async fn load_schema(database_url: &str) -> Result<DatabaseSchema> {
        tracing::info!("Loading schema from {}", database_url);

        Ok(DatabaseSchema {
            database_name: "sample_db".into(),

            tables: vec![
                Table {
                    name: "customers".into(),

                    columns: vec![
                        Column {
                            name: "customer_id".into(),
                            data_type: "BIGINT".into(),
                            nullable: false,
                        },
                        Column {
                            name: "email".into(),
                            data_type: "VARCHAR".into(),
                            nullable: true,
                        },
                    ],
                },
                Table {
                    name: "orders".into(),

                    columns: vec![
                        Column {
                            name: "order_id".into(),
                            data_type: "BIGINT".into(),
                            nullable: false,
                        },
                        Column {
                            name: "customer_id".into(),
                            data_type: "BIGINT".into(),
                            nullable: false,
                        },
                    ],
                },
            ],
        })
    }
}
