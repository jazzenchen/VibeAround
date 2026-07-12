use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use super::threads::runtime::ThreadRuntime;
use super::threads::store::WorkspaceThreadId;

pub(super) struct RuntimeRegistry {
    command_tx: mpsc::UnboundedSender<RuntimeRegistryCommand>,
}

enum RuntimeRegistryCommand {
    Get {
        thread_id: WorkspaceThreadId,
        reply: oneshot::Sender<Option<Arc<ThreadRuntime>>>,
    },
    GetOrInsert {
        thread_id: WorkspaceThreadId,
        runtime: Arc<ThreadRuntime>,
        reply: oneshot::Sender<Arc<ThreadRuntime>>,
    },
    Remove {
        thread_id: WorkspaceThreadId,
        expected: Option<Arc<ThreadRuntime>>,
    },
    Entries(oneshot::Sender<Vec<(WorkspaceThreadId, Arc<ThreadRuntime>)>>),
    Values(oneshot::Sender<Vec<Arc<ThreadRuntime>>>),
}

impl RuntimeRegistry {
    pub(super) fn new() -> Self {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut runtimes = HashMap::<WorkspaceThreadId, Arc<ThreadRuntime>>::new();
            while let Some(command) = command_rx.recv().await {
                match command {
                    RuntimeRegistryCommand::Get { thread_id, reply } => {
                        let _ = reply.send(runtimes.get(&thread_id).cloned());
                    }
                    RuntimeRegistryCommand::GetOrInsert {
                        thread_id,
                        runtime,
                        reply,
                    } => {
                        let runtime = Arc::clone(runtimes.entry(thread_id).or_insert(runtime));
                        let _ = reply.send(runtime);
                    }
                    RuntimeRegistryCommand::Remove {
                        thread_id,
                        expected,
                    } => {
                        let remove = expected.as_ref().is_none_or(|expected| {
                            runtimes
                                .get(&thread_id)
                                .is_some_and(|current| Arc::ptr_eq(current, expected))
                        });
                        if remove {
                            runtimes.remove(&thread_id);
                        }
                    }
                    RuntimeRegistryCommand::Entries(reply) => {
                        let _ = reply.send(
                            runtimes
                                .iter()
                                .map(|(id, runtime)| (id.clone(), Arc::clone(runtime)))
                                .collect(),
                        );
                    }
                    RuntimeRegistryCommand::Values(reply) => {
                        let _ = reply.send(runtimes.values().cloned().collect());
                    }
                }
            }
        });
        Self { command_tx }
    }

    pub(super) async fn get(&self, thread_id: &WorkspaceThreadId) -> Option<Arc<ThreadRuntime>> {
        let (reply, result) = oneshot::channel();
        self.command_tx
            .send(RuntimeRegistryCommand::Get {
                thread_id: thread_id.clone(),
                reply,
            })
            .expect("runtime registry owner must outlive its handle");
        result.await.expect("runtime registry owner must reply")
    }

    pub(super) async fn get_or_insert(
        &self,
        thread_id: WorkspaceThreadId,
        runtime: Arc<ThreadRuntime>,
    ) -> Arc<ThreadRuntime> {
        let (reply, result) = oneshot::channel();
        self.command_tx
            .send(RuntimeRegistryCommand::GetOrInsert {
                thread_id,
                runtime,
                reply,
            })
            .expect("runtime registry owner must outlive its handle");
        result.await.expect("runtime registry owner must reply")
    }

    pub(super) fn remove(&self, thread_id: WorkspaceThreadId) {
        let _ = self.command_tx.send(RuntimeRegistryCommand::Remove {
            thread_id,
            expected: None,
        });
    }

    pub(super) fn remove_if_current(
        &self,
        thread_id: WorkspaceThreadId,
        runtime: Arc<ThreadRuntime>,
    ) {
        let _ = self.command_tx.send(RuntimeRegistryCommand::Remove {
            thread_id,
            expected: Some(runtime),
        });
    }

    pub(super) async fn entries(&self) -> Vec<(WorkspaceThreadId, Arc<ThreadRuntime>)> {
        let (reply, result) = oneshot::channel();
        self.command_tx
            .send(RuntimeRegistryCommand::Entries(reply))
            .expect("runtime registry owner must outlive its handle");
        result.await.expect("runtime registry owner must reply")
    }

    pub(super) async fn values(&self) -> Vec<Arc<ThreadRuntime>> {
        let (reply, result) = oneshot::channel();
        self.command_tx
            .send(RuntimeRegistryCommand::Values(reply))
            .expect("runtime registry owner must outlive its handle");
        result.await.expect("runtime registry owner must reply")
    }
}
