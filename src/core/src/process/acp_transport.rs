//! ACP stdio transport helpers.
//!
//! The upstream `ByteStreams` transport treats EOF as a clean background
//! completion and keeps waiting for the foreground task. Supervised child
//! processes need a stronger lifecycle signal: stdout EOF means the bridge
//! should finish so the supervisor can observe the child exit. This wrapper
//! keeps the SDK's line transport and adds an explicit EOF notification.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use agent_client_protocol as acp;
use futures::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader};
use futures::{Sink, Stream};
use tokio::sync::oneshot;

/// Local stdio writes should complete immediately. If the child keeps
/// producing heartbeats but stops reading stdin, bound the SDK's unbounded
/// outgoing queue by failing the bridge once the OS pipe remains full.
const STDIO_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn notifying_stdio_transport<OB, IB>(
    outgoing: OB,
    incoming: IB,
) -> (
    acp::Lines<
        impl Sink<String, Error = io::Error> + Send + 'static,
        impl Stream<Item = io::Result<String>> + Send + 'static,
    >,
    oneshot::Receiver<()>,
)
where
    OB: AsyncWrite + Send + 'static,
    IB: AsyncRead + Send + 'static,
{
    let (closed_tx, closed_rx) = oneshot::channel();

    let outgoing_sink =
        futures::sink::unfold(Box::pin(outgoing), async move |mut writer, line: String| {
            write_line_with_timeout(&mut writer, line, STDIO_WRITE_TIMEOUT).await?;
            Ok::<_, io::Error>(writer)
        });

    let incoming_lines = BufReader::new(incoming).lines();
    let incoming_lines = NotifyOnEnd {
        inner: Box::pin(incoming_lines),
        closed_tx: Some(closed_tx),
    };

    (acp::Lines::new(outgoing_sink, incoming_lines), closed_rx)
}

async fn write_line_with_timeout<W>(
    writer: &mut W,
    line: String,
    timeout: Duration,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    use futures::AsyncWriteExt;

    let mut bytes = line.into_bytes();
    bytes.push(b'\n');
    tokio::time::timeout(timeout, writer.write_all(&bytes))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "ACP stdio write timed out"))?
}

struct NotifyOnEnd<S> {
    inner: Pin<Box<S>>,
    closed_tx: Option<oneshot::Sender<()>>,
}

impl<S> Unpin for NotifyOnEnd<S> {}

impl<S> Stream for NotifyOnEnd<S>
where
    S: Stream<Item = io::Result<String>>,
{
    type Item = io::Result<String>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(None) => {
                if let Some(tx) = this.closed_tx.take() {
                    let _ = tx.send(());
                }
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StalledWriter;

    impl AsyncWrite for StalledWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Pending
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn stalled_stdio_write_times_out() {
        let mut writer = StalledWriter;
        let error =
            write_line_with_timeout(&mut writer, "{}".to_string(), Duration::from_millis(10))
                .await
                .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }
}
