use crate::error::InfraResult;
use tokio::sync::oneshot;

pub(super) fn send_init_result(
    init_tx: &std::sync::mpsc::SyncSender<InfraResult<()>>,
    result: InfraResult<()>,
    context: &'static str,
) {
    if let Err(err) = &result {
        tracing::error!(context = context, error = %err, "sqlite worker init failed");
    }
    if init_tx.send(result).is_err() {
        tracing::warn!(context = context, "sqlite worker init receiver dropped");
    }
}

pub(super) fn send_response<T>(
    command: &'static str,
    resp: oneshot::Sender<InfraResult<T>>,
    result: InfraResult<T>,
) {
    if resp.send(result).is_err() {
        tracing::warn!(command = command, "sqlite response receiver dropped");
    }
}
