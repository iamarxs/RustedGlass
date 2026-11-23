use super::{DecodeRequest, Event, Lane, OpenedOutcome, Wake};
use crate::decode::{self, AnimFrame, Opened};

/// The decode lane. [`decode::Animation`] — the `image` frame iterator, which is not
/// `Send` — is constructed here and never leaves this thread.
pub(super) fn run(requests: &Lane<DecodeRequest>, events: &Lane<Event>, wake: &Wake) {
    // The streaming cursor of the accepted selection; a new selection clears it.
    let mut stream: Option<(u64, decode::Animation)> = None;
    while let Some(request) = requests.pop() {
        match request {
            DecodeRequest::Open { generation, path } => {
                let identity = crate::cache::stat(&path);
                let outcome = match decode::open(&path) {
                    Ok(Opened::Still(image)) => {
                        stream = None;
                        OpenedOutcome::Still { image }
                    }
                    Ok(Opened::Animated(animation, first)) => {
                        stream = Some((generation, animation));
                        OpenedOutcome::AnimatedFirst { frame: first }
                    }
                    Err(error) => {
                        stream = None;
                        OpenedOutcome::Failed { error }
                    }
                };
                wake.deliver(
                    events,
                    Event::Opened {
                        generation,
                        identity,
                        outcome,
                    },
                );
            }
            DecodeRequest::NextFrame { generation } => {
                if let Some(animation) = alive(&mut stream, generation) {
                    let frame = animation.next_frame();
                    deliver_frame(wake, events, &mut stream, generation, frame);
                }
            }
            DecodeRequest::Rewind { generation } => {
                if let Some(animation) = alive(&mut stream, generation) {
                    let frame = animation.rewind().and_then(|_| animation.next_frame());
                    deliver_frame(wake, events, &mut stream, generation, frame);
                }
            }
        }
    }
}

/// The animation cursor of `generation`, or `None` when the selection has moved on or
/// nothing is streaming: a stale request touches nothing.
fn alive(
    stream: &mut Option<(u64, decode::Animation)>,
    generation: u64,
) -> Option<&mut decode::Animation> {
    let (live, animation) = stream.as_mut()?;
    (*live == generation).then_some(animation)
}

/// Delivers a streamed or rewound frame. At the end of the animation nothing is dropped:
/// the UI decides between rewind and navigation. On a read failure the cursor is dropped
/// instead, because the UI has no use for it without the file behind it.
fn deliver_frame(
    wake: &Wake,
    events: &Lane<Event>,
    stream: &mut Option<(u64, decode::Animation)>,
    generation: u64,
    frame: Result<Option<AnimFrame>, String>,
) {
    match frame {
        Ok(Some(frame)) => {
            wake.deliver(events, Event::Frame { generation, frame });
        }
        Ok(None) => {
            wake.deliver(events, Event::FrameEnded { generation });
        }
        Err(_) => {
            *stream = None;
            wake.deliver(events, Event::FrameFailed { generation });
        }
    }
}
