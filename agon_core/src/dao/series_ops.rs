//! Match series operations: create, get. Generating a series' occurrences
//! into independent matches is not part of this module — it belongs to the
//! workflow that drives `occurrences_generated` forward, which lands in a
//! later change.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::put_item::PutItemError;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::records::SeriesRecord;

pub const TYPE_SERIES: &str = "series";

impl Dao {
    /// Create a series meta item. `Conflict` if the series id already exists.
    #[tracing::instrument(skip(self, series), fields(series_id = %series.id))]
    pub async fn create_series(&self, series: &SeriesRecord) -> DaoResult<()> {
        let item = to_item(
            &Pk::Series(series.id.clone()),
            &Sk::Meta,
            TYPE_SERIES,
            series,
        )?;

        let result = self
            .client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_put_conditional_failure(&e) => Err(DaoError::Conflict(format!(
                "series {} already exists",
                series.id
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch a series by id. `None` if absent.
    #[tracing::instrument(skip(self))]
    pub async fn get_series(&self, series_id: &str) -> DaoResult<Option<SeriesRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Series(series_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }
}

fn is_put_conditional_failure(err: &SdkError<PutItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(
                se.err(),
                PutItemError::ConditionalCheckFailedException(_)
            )
    )
}
