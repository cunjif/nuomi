//! Kernel: ordered plugin lifecycle management.

use std::sync::Arc;

use super::{Context, HarnessError, Plugin};

/// Owns the shared [`Context`] and drives plugin init → start → dispose.
pub struct Kernel {
    ctx: Context,
    plugins: Vec<Arc<dyn Plugin>>,
    started: Vec<String>,
}

impl Kernel {
    pub fn new(ctx: Context) -> Self {
        Self {
            ctx,
            plugins: Vec::new(),
            started: Vec::new(),
        }
    }

    pub fn context(&self) -> &Context {
        &self.ctx
    }

    /// Registers a plugin. Order matters: init runs in registration order.
    pub fn register(&mut self, plugin: Arc<dyn Plugin>) -> Result<(), HarnessError> {
        if self.plugins.iter().any(|p| p.id() == plugin.id()) {
            return Err(HarnessError::DuplicateService {
                name: format!("plugin#{}", plugin.id()),
                owner: "kernel".to_string(),
            });
        }
        self.plugins.push(plugin);
        Ok(())
    }

    /// Runs `init` for every plugin (registration order), then `start`
    /// for each successfully initialized one.
    pub async fn boot(&mut self) -> Result<(), HarnessError> {
        for plugin in &self.plugins {
            let phase = "init";
            plugin
                .init(&self.ctx)
                .await
                .map_err(|e| HarnessError::PluginFailed {
                    plugin: plugin.id().to_string(),
                    phase,
                    message: e.to_string(),
                })?;
            let phase = "start";
            plugin
                .start()
                .await
                .map_err(|e| HarnessError::PluginFailed {
                    plugin: plugin.id().to_string(),
                    phase,
                    message: e.to_string(),
                })?;
            self.started.push(plugin.id().to_string());
        }
        Ok(())
    }

    /// Disposes started plugins in reverse order; collects but does not
    /// abort on individual failures so remaining plugins still tear down.
    pub async fn shutdown(&mut self) -> Vec<HarnessError> {
        let mut errors = Vec::new();
        while let Some(id) = self.started.pop() {
            if let Some(p) = self.plugins.iter().find(|p| p.id() == id) {
                if let Err(e) = p.dispose().await {
                    errors.push(HarnessError::PluginFailed {
                        plugin: id.clone(),
                        phase: "dispose",
                        message: e.to_string(),
                    });
                }
            }
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl Recorder {
        fn push(&self, s: &str) {
            self.0.lock().unwrap().push(s.to_string());
        }
    }

    struct FakePlugin {
        id: &'static str,
        log: Arc<Recorder>,
    }

    #[async_trait]
    impl Plugin for FakePlugin {
        fn id(&self) -> &str {
            self.id
        }
        async fn init(&self, _ctx: &Context) -> Result<(), HarnessError> {
            self.log.push(&format!("init:{}", self.id));
            Ok(())
        }
        async fn start(&self) -> Result<(), HarnessError> {
            self.log.push(&format!("start:{}", self.id));
            Ok(())
        }
        async fn dispose(&self) -> Result<(), HarnessError> {
            self.log.push(&format!("dispose:{}", self.id));
            Ok(())
        }
    }

    #[tokio::test]
    async fn boots_in_order_and_disposes_in_reverse() {
        let log = Arc::new(Recorder::default());
        let mut kernel = Kernel::new(Context::default());
        kernel
            .register(Arc::new(FakePlugin {
                id: "a",
                log: log.clone(),
            }))
            .unwrap();
        kernel
            .register(Arc::new(FakePlugin {
                id: "b",
                log: log.clone(),
            }))
            .unwrap();
        kernel.boot().await.unwrap();
        kernel.shutdown().await;
        let events = log.0.lock().unwrap().clone();
        assert_eq!(
            events,
            vec![
                "init:a",
                "start:a",
                "init:b",
                "start:b",
                "dispose:b",
                "dispose:a"
            ]
        );
    }

    #[tokio::test]
    async fn duplicate_plugin_id_rejected() {
        let log = Arc::new(Recorder::default());
        let mut kernel = Kernel::new(Context::default());
        kernel
            .register(Arc::new(FakePlugin {
                id: "x",
                log: log.clone(),
            }))
            .unwrap();
        assert!(matches!(
            kernel.register(Arc::new(FakePlugin { id: "x", log })),
            Err(HarnessError::DuplicateService { .. })
        ));
    }

    struct FailingInit;

    #[async_trait]
    impl Plugin for FailingInit {
        fn id(&self) -> &str {
            "bad"
        }
        async fn init(&self, _ctx: &Context) -> Result<(), HarnessError> {
            Err(HarnessError::PluginNotFound("boom".into()))
        }
    }

    #[tokio::test]
    async fn init_failure_is_wrapped_with_phase() {
        let mut kernel = Kernel::new(Context::default());
        kernel.register(Arc::new(FailingInit)).unwrap();
        let err = kernel.boot().await.unwrap_err();
        assert!(err.to_string().contains("during init"), "{err}");
    }
}
