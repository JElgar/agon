//! Asset operations: create (pending), get, mark uploaded/failed.
//!
//! The `pending → uploaded` transition is normally driven by the async storage
//! event worker, not by request handlers. The presigned upload target is built
//! at the API layer from `storage_key`; it is not stored.

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::records::AssetRecord;

pub const TYPE_ASSET: &str = "asset";

impl Dao {
    /// Create a new (pending) asset.
    pub async fn create_asset(&self, asset: &AssetRecord) -> DaoResult<()> {
        let item = to_item(&Pk::Asset(asset.id.clone()), &Sk::Meta, TYPE_ASSET, asset)?;
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .send()
            .await
            .map_err(|e| classify_put_conflict(e, &asset.id))?;
        Ok(())
    }

    /// Fetch an asset by id. `None` if absent.
    pub async fn get_asset(&self, asset_id: &str) -> DaoResult<Option<AssetRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Asset(asset_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Mark an asset uploaded and set its public URL (storage-event driven).
    pub async fn mark_asset_uploaded(&self, asset_id: &str, url: &str) -> DaoResult<()> {
        self.set_asset_status(asset_id, "uploaded", Some(url)).await
    }

    /// Mark an asset failed (e.g. upload expired / rejected).
    pub async fn mark_asset_failed(&self, asset_id: &str) -> DaoResult<()> {
        self.set_asset_status(asset_id, "failed", None).await
    }

    async fn set_asset_status(
        &self,
        asset_id: &str,
        status: &str,
        url: Option<&str>,
    ) -> DaoResult<()> {
        let mut update = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Asset(asset_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#status", "status")
            .expression_attribute_values(":status", s(status));

        update = match url {
            Some(u) => update
                .update_expression("SET #status = :status, #url = :url")
                .expression_attribute_names("#url", "url")
                .expression_attribute_values(":url", s(u)),
            None => update.update_expression("SET #status = :status"),
        };

        match update.send().await {
            Ok(_) => Ok(()),
            Err(e)
                if matches!(
                    &e,
                    aws_sdk_dynamodb::error::SdkError::ServiceError(se)
                        if matches!(
                            se.err(),
                            aws_sdk_dynamodb::operation::update_item::UpdateItemError::ConditionalCheckFailedException(_)
                        )
                ) =>
            {
                Err(DaoError::NotFound(format!("asset {asset_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }
}

fn classify_put_conflict(
    err: aws_sdk_dynamodb::error::SdkError<aws_sdk_dynamodb::operation::put_item::PutItemError>,
    asset_id: &str,
) -> DaoError {
    match &err {
        aws_sdk_dynamodb::error::SdkError::ServiceError(se)
            if matches!(
                se.err(),
                aws_sdk_dynamodb::operation::put_item::PutItemError::ConditionalCheckFailedException(_)
            ) =>
        {
            DaoError::Conflict(format!("asset {asset_id} already exists"))
        }
        _ => DaoError::Dynamo(err.to_string()),
    }
}
