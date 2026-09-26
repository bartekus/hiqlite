use crate::Error;
pub use cryptr::EncKeys;
pub use cryptr::stream::s3::*;
use cryptr::stream::writer::channel_writer::{ChannelReceiver, ChannelWriter};
use cryptr::{EncValue, FileReader, FileWriter, S3Reader, S3Writer, StreamReader, StreamWriter};
use std::env;
use std::sync::Arc;
use tokio::task;

#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: Bucket,
}

impl S3Config {
    pub fn new<S>(
        endpoint: &str,
        bucket_name: S,
        region: S,
        key: S,
        secret: S,
        path_style: bool,
    ) -> Result<Arc<Self>, Error>
    where
        S: Into<String>,
    {
        let endpoint = reqwest::Url::parse(endpoint).map_err(|err| Error::S3(err.to_string()))?;
        let region = Region(region.into());
        let credentials = Credentials {
            access_key_id: AccessKeyId(key.into()),
            access_key_secret: AccessKeySecret(secret.into()),
        };
        let options = Some(BucketOptions {
            path_style,
            list_objects_v2: true,
        });
        let bucket = Bucket::new(endpoint, bucket_name.into(), region, credentials, options)
            .map_err(|err| Error::S3(err.to_string()))?;

        // The credentials are not proven here, because this constructor is synchronous.
        // `verify_access` is the async probe that does it, and the restore path calls it.

        Ok(Arc::new(Self { bucket }))
    }

    /// Read the S3 configuration from the environment, naming what is wrong.
    ///
    /// Every failure here used to be an `expect` or an `unwrap`, on the authored assumption
    /// that "all values exist when we can read the url successfully". A single missing or
    /// misspelled variable therefore ended the process at configuration time, while the same
    /// `Bucket::new` failure in [`Self::new`] is a returned `Error::S3`.
    ///
    /// `HQL_S3_PATH_STYLE` is now optional and defaults to `true`, which is what every
    /// self-hosted S3-compatible store needs; it was the one variable with an obvious default
    /// and no reason to be mandatory.
    pub fn try_from_env_checked() -> Result<Option<Arc<Self>>, Error> {
        Self::from_lookup(&|name| env::var(name).ok())
    }

    /// The same configuration read through an arbitrary lookup.
    ///
    /// Separated from the environment so it can be tested without process-wide mutation, which
    /// `009` D-3 records as the reason no environment route has a test. The lookup is the only
    /// thing that differs.
    pub fn from_lookup(
        lookup: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Option<Arc<Self>>, Error> {
        let Some(url) = lookup("HQL_S3_URL") else {
            return Ok(None);
        };

        let required = |name: &str| -> Result<String, Error> {
            lookup(name).ok_or_else(|| {
                Error::Config(
                    format!("HQL_S3_URL is set, so {name} is required and is missing").into(),
                )
            })
        };

        let url = reqwest::Url::parse(&url)
            .map_err(|err| Error::Config(format!("HQL_S3_URL is not a valid URL: {err}").into()))?;
        let bucket_name = required("HQL_S3_BUCKET")?;
        let region = Region(required("HQL_S3_REGION")?);
        let path_style = match lookup("HQL_S3_PATH_STYLE") {
            Some(v) => v.trim().parse::<bool>().map_err(|err| {
                Error::Config(format!("HQL_S3_PATH_STYLE must be `true` or `false`: {err}").into())
            })?,
            None => true,
        };
        let credentials = Credentials {
            access_key_id: AccessKeyId(required("HQL_S3_KEY")?),
            access_key_secret: AccessKeySecret(required("HQL_S3_SECRET")?),
        };

        let options = Some(BucketOptions {
            path_style,
            list_objects_v2: true,
        });
        let bucket = Bucket::new(url, bucket_name, region, credentials, options)
            .map_err(|err| Error::S3(err.to_string()))?;

        Ok(Some(Arc::new(S3Config { bucket })))
    }

    /// The infallible wrapper the environment-configuration path still uses.
    ///
    /// The message it fails with is the named one from [`Self::try_from_env_checked`], so an
    /// operator is told which variable is wrong rather than which `expect` fired. Removing the
    /// panic itself means making `NodeConfig::from_env` fallible, which is a public API change
    /// and belongs to the startup-error work, not here.
    pub fn try_from_env() -> Option<Arc<Self>> {
        match Self::try_from_env_checked() {
            Ok(cfg) => cfg,
            Err(err) => panic!("Invalid S3 configuration: {err}"),
        }
    }

    /// Prove the credentials work, before anything depends on them.
    ///
    /// Without this, a wrong key is first discovered by the detached upload task in
    /// `create_backup`, **after** the backup has been acknowledged, as an error log. One list
    /// call is enough to turn that into a failure the caller sees.
    pub async fn verify_access(&self) -> Result<(), Error> {
        self.bucket.list("", None).await.map_err(|err| {
            Error::S3(format!(
                "cannot access the configured S3 bucket '{}': {err}",
                self.bucket.name
            ))
        })?;
        Ok(())
    }

    pub(crate) async fn push(&self, path: &str, object: &str) -> Result<(), Error> {
        let reader = StreamReader::File(FileReader {
            path,
            print_progress: false,
        });
        let writer = StreamWriter::S3(S3Writer {
            bucket: &self.bucket,
            object,
        });

        EncValue::encrypt_stream(reader, writer)
            .await
            .map_err(|err| Error::S3(err.to_string()))
    }

    pub(crate) async fn pull(&self, object: &str, path: &str) -> Result<(), Error> {
        let reader = StreamReader::S3(S3Reader {
            bucket: &self.bucket,
            object,
            print_progress: false,
        });
        let writer = StreamWriter::File(FileWriter {
            path,
            overwrite_target: true,
        });

        EncValue::decrypt_stream(reader, writer)
            .await
            .map_err(|err| Error::S3(err.to_string()))
    }

    pub(crate) fn pull_channel(&self, object: String) -> Result<ChannelReceiver, Error> {
        let (channel_writer, rx) = ChannelWriter::new();
        let writer = StreamWriter::Channel(channel_writer.clone());

        let bucket = self.bucket.clone();
        task::spawn(async move {
            let reader = StreamReader::S3(S3Reader {
                bucket: &bucket,
                object: &object,
                print_progress: false,
            });

            if let Err(err) = EncValue::decrypt_stream(reader, writer).await {
                channel_writer.err(Some(err)).await;
            }
        });

        Ok(rx)
    }
}
