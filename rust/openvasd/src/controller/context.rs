// SPDX-FileCopyrightText: 2023 Greenbone AG
//
// SPDX-License-Identifier: GPL-2.0-or-later

use std::{path::PathBuf, sync::RwLock};

use async_trait::async_trait;
use storage::DefaultDispatcher;

use crate::{
    response,
    scan::{Error, ScanDeleter, ScanResultFetcher, ScanStarter, ScanStopper},
};

#[derive(Debug, Clone)]
pub struct NoScanner;
#[derive(Debug, Clone)]
pub struct Scanner<S>(S);

#[derive(Debug, Clone)]
/// sets the interval when to check for new results.
pub struct ResultContext(pub std::time::Duration);

impl From<std::time::Duration> for ResultContext {
    fn from(d: std::time::Duration) -> Self {
        Self(d)
    }
}

#[derive(Debug, Clone)]
/// Sets the path to the feed and the interval when to check for updates.
pub struct FeedContext {
    /// The path to the feed.
    pub path: PathBuf,
    /// The interval when to check for updates.
    pub verify_interval: std::time::Duration,
}

impl From<(PathBuf, std::time::Duration)> for FeedContext {
    fn from((path, verify_interval): (PathBuf, std::time::Duration)) -> Self {
        Self {
            path,
            verify_interval,
        }
    }
}

impl From<(&str, std::time::Duration)> for FeedContext {
    fn from((path, verify_interval): (&str, std::time::Duration)) -> Self {
        (PathBuf::from(path), verify_interval).into()
    }
}

#[derive(Debug, Clone, Default)]
/// Context builder is used to build the context of the application.
pub struct ContextBuilder<S, DB, T> {
    scanner: T,
    storage: DB,
    result_config: Option<ResultContext>,
    feed_config: Option<FeedContext>,
    api_key: Option<String>,
    enable_get_scans: bool,
    marker: std::marker::PhantomData<S>,
    response: response::Response,
}
impl<S> ContextBuilder<S, crate::storage::InMemoryStorage<crate::crypt::ChaCha20Crypt>, NoScanner> {
    /// Creates a new context builder.
    pub fn new() -> Self {
        Self {
            scanner: NoScanner,
            storage: crate::storage::InMemoryStorage::default(),
            result_config: None,
            feed_config: None,
            api_key: None,
            marker: std::marker::PhantomData,
            enable_get_scans: false,
            response: response::Response::default(),
        }
    }
}

impl<S, DB, T> ContextBuilder<S, DB, T> {
    /// Sets the result config.
    pub fn result_config(mut self, config: impl Into<ResultContext>) -> Self {
        self.result_config = Some(config.into());
        self
    }

    /// Sets the feed config.
    pub fn feed_config(mut self, config: impl Into<FeedContext>) -> Self {
        self.feed_config = Some(config.into());
        if let Some(fp) = self.feed_config.as_ref() {
            let loader = nasl_interpreter::FSPluginLoader::new(fp.path.clone());
            let dispatcher: DefaultDispatcher<String> = DefaultDispatcher::default();
            let version = feed::version(&loader, &dispatcher).unwrap();
            self.response.set_feed_version(&version);
        }
        self
    }

    /// Sets the api key.
    pub fn api_key(mut self, api_key: impl Into<Option<String>>) -> Self {
        self.api_key = api_key.into();
        if self.api_key.is_some() {
            self.response.add_authentication("x-api-key");
        }
        self
    }

    /// Enables the GET /scans endpoint.
    pub fn enable_get_scans(mut self, enable: bool) -> Self {
        self.enable_get_scans = enable;
        self
    }

    /// Sets the storage.
    #[allow(dead_code)]
    pub fn storage(mut self, storage: DB) -> Self {
        self.storage = storage;
        self
    }
}

impl<S, DB> ContextBuilder<S, DB, NoScanner>
where
    S: Clone,
{
    /// Sets the scanner. This is required.
    pub fn scanner(self, scanner: S) -> ContextBuilder<S, DB, Scanner<S>>
    where
        S: super::Scanner + 'static + std::marker::Send + std::marker::Sync + std::fmt::Debug,
    {
        let Self {
            result_config,
            feed_config,
            api_key,
            enable_get_scans,
            scanner: _,
            marker: _,
            response,
            storage,
        } = self;
        ContextBuilder {
            scanner: Scanner(scanner),
            storage,
            result_config,
            feed_config,
            marker: std::marker::PhantomData,
            api_key,
            enable_get_scans,
            response,
        }
    }
}

impl<S, DB> ContextBuilder<S, DB, Scanner<S>> {
    pub fn build(self) -> Context<S, DB> {
        Context {
            scanner: self.scanner.0,
            response: self.response,
            db: self.storage,
            oids: Default::default(),
            result_config: self.result_config,
            feed_config: self.feed_config,
            abort: Default::default(),
            api_key: self.api_key,
            enable_get_scans: self.enable_get_scans,
        }
    }
}

#[derive(Debug)]
/// The context of the application
pub struct Context<S, DB> {
    /// The scanner that is used to start, stop and fetch results of scans.
    pub scanner: S,
    /// Creates responses
    pub response: response::Response,
    /// The scans that are being tracked.
    ///
    /// It is locked to allow concurrent access, usually the results are updated
    /// with a background task and appended to the progress of the scan.
    pub db: DB,
    /// The OIDs thate can be handled by this sensor.
    pub oids: RwLock<(String, Vec<String>)>,
    /// Configuration for result fetching
    pub result_config: Option<ResultContext>,
    /// Configuration for feed handling.
    pub feed_config: Option<FeedContext>,
    /// The api key that is used to authenticate the client.
    ///
    /// When none api key is set, no authentication is required.
    pub api_key: Option<String>,
    /// Whether to enable the GET /scans endpoint
    pub enable_get_scans: bool,
    /// Aborts the background loops
    pub abort: RwLock<bool>,
}

#[derive(Debug, Clone, Default)]
/// A scanner without any side effects. Used for testing.
pub struct NoOpScanner;

#[async_trait]
impl ScanStarter for NoOpScanner {
    async fn start_scan(&self, _: models::Scan) -> Result<(), Error> {
        Ok(())
    }
}

#[async_trait]
impl ScanStopper for NoOpScanner {
    async fn stop_scan<I>(&self, _: I) -> Result<(), Error>
    where
        I: AsRef<str> + Send,
    {
        Ok(())
    }
}

#[async_trait]
impl ScanDeleter for NoOpScanner {
    async fn delete_scan<I>(&self, _: I) -> Result<(), Error>
    where
        I: AsRef<str> + Send,
    {
        Ok(())
    }
}

#[async_trait]
impl ScanResultFetcher for NoOpScanner {
    async fn fetch_results<I>(&self, _: I) -> Result<crate::scan::FetchResult, Error>
    where
        I: AsRef<str> + Send,
    {
        Ok(Default::default())
    }
}

impl Default
    for Context<NoOpScanner, crate::storage::InMemoryStorage<crate::crypt::ChaCha20Crypt>>
{
    fn default() -> Self {
        ContextBuilder::new().scanner(Default::default()).build()
    }
}
