use aws_sdk_dynamodb::Client;

/// Handle to the DynamoDB single table. Cheap to clone (the SDK client is an
/// `Arc` internally); pass by shared reference or clone freely.
#[derive(Clone)]
pub struct Dao {
    pub(super) client: Client,
    pub(super) table: String,
}

impl Dao {
    /// Build a DAO from an existing SDK client and the table name.
    pub fn new(client: Client, table: impl Into<String>) -> Self {
        Self {
            client,
            table: table.into(),
        }
    }

    /// Build a DAO using the ambient AWS config (env / profile / IMDS). The
    /// table name typically comes from `AGON_TABLE_NAME`.
    pub async fn from_env(table: impl Into<String>) -> Self {
        let config = aws_config::load_from_env().await;
        Self::new(Client::new(&config), table)
    }

    pub(super) fn table(&self) -> &str {
        &self.table
    }
}
