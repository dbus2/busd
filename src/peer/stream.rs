#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::{pin::Pin, sync::Arc};

use anyhow::{bail, Error, Result};
use futures_util::{Stream as FutureStream, TryStream, TryStreamExt};
use tracing::trace;
use zbus::{
    zvariant::Type, Message, MessageBuilder, MessageField, MessageFieldCode, MessageStream,
    MessageType,
};

use crate::peer::Peer;

/// Message stream for a peer.
///
/// This stream ensures the following for each message produced:
///
/// * The destination field is present and readable for non-signals.
/// * The sender field is present and set to the unique name of the peer.
pub struct Stream {
    stream: Pin<Box<StreamInner>>,
}

type StreamInner =
    dyn TryStream<Ok = Arc<Message>, Error = Error, Item = Result<Arc<Message>>> + Send;

impl Stream {
    pub fn for_peer(peer: &Peer) -> Self {
        let unique_name = peer.unique_name().clone();
        let stream = MessageStream::from(peer.conn())
            .map_err(Into::into)
            .and_then(move |msg| {
                let unique_name = unique_name.clone();
                async move {
                    let fields = match msg.message_type() {
                        MessageType::MethodCall
                        | MessageType::MethodReturn
                        | MessageType::Error
                        | MessageType::Signal => msg.fields()?,
                        MessageType::Invalid => bail!("Invalid message"),
                    };

                    // Ensure destination field is present and readable for non-signals.
                    if msg.message_type() != MessageType::Signal {
                        match fields.get_field(MessageFieldCode::Destination) {
                            Some(MessageField::Destination(_)) => (),
                            Some(_) => {
                                bail!("failed to parse message: Invalid destination field");
                            }
                            None => bail!("missing destination field"),
                        }
                    }

                    // Ensure sender field is present. If it is not we add it using the unique name
                    // of the peer.
                    match fields.get_field(MessageFieldCode::Sender) {
                        Some(MessageField::Sender(sender)) if *sender == unique_name => Ok(msg),
                        Some(_) => bail!("failed to parse message: Invalid sender field"),
                        None => {
                            let header = msg.header()?;
                            let signature = match header.signature()? {
                                Some(sig) => sig.clone(),
                                None => <()>::signature(),
                            };
                            let body_bytes = msg.body_as_bytes()?;
                            let builder =
                                MessageBuilder::from(header.clone()).sender(&unique_name)?;
                            let new_msg = unsafe {
                                builder.build_raw_body(
                                    body_bytes,
                                    signature,
                                    #[cfg(unix)]
                                    msg.take_fds().iter().map(|fd| fd.as_raw_fd()).collect(),
                                )?
                            };
                            trace!("Added sender field to message: {:?}", new_msg);

                            Ok(Arc::new(new_msg))
                        }
                    }
                }
            });

        Self {
            stream: Box::pin(stream),
        }
    }
}

impl FutureStream for Stream {
    type Item = Result<Arc<Message>>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<Option<Result<Arc<Message>>>> {
        FutureStream::poll_next(Pin::new(&mut self.get_mut().stream), cx)
    }
}
