use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bitvec::{field::BitField as _, order::Msb0, slice::BitSlice, view::BitView as _};
use futures::Stream;
use pin_project::pin_project;
use rubikmaster::{Command, Move};
use tokio::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DecodeError {
    InvalidLength,
    UnknownEventType,
}

#[derive(Debug)]
pub struct GanCubeMove {
    pub serial: u8,
    pub face: rubikmaster::Move,
    pub prime: bool,
    pub elapsed: Duration,
}

impl GanCubeMove {
    pub fn command(&self) -> Command {
        Command(self.face, if self.prime { -1 } else { 1 })
    }

    fn from_bits(bits: &BitSlice<u8, Msb0>) -> Result<Self, DecodeError> {
        if bits.len() < 63 {
            return Err(DecodeError::InvalidLength);
        }

        let serial = bits[4..12].load_be::<u8>();
        let face = match bits[12..16].load_be::<u8>() {
            0 => Move::U,
            1 => Move::R,
            2 => Move::F,
            3 => Move::D,
            4 => Move::L,
            5 => Move::B,
            _ => unreachable!(),
        };
        let prime = bits[16];
        let elapsed = Duration::from_millis(bits[47..63].load_be::<u16>() as u64);

        Ok(Self { serial, face, prime, elapsed })
    }
}

#[derive(Debug)]
pub enum GanCubeEvent {
    Move(GanCubeMove),
}

impl GanCubeEvent {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        Self::from_bits(bytes.view_bits::<Msb0>())
    }

    fn from_bits(bits: &BitSlice<u8, Msb0>) -> Result<Self, DecodeError> {
        let event_type = bits[0..4].load::<u8>();
        match event_type {
            2 => Ok(GanCubeEvent::Move(GanCubeMove::from_bits(bits)?)),
            _ => Err(DecodeError::UnknownEventType),
        }
    }
}

#[pin_project]
pub struct DecoderStream<S: Stream<Item = T>, T: AsRef<[u8]>> {
    #[pin]
    inner: S,
}

impl<S: Stream<Item = T>, T: AsRef<[u8]>> DecoderStream<S, T> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: Stream<Item = T>, T: AsRef<[u8]>> Stream for DecoderStream<S, T> {
    type Item = Result<GanCubeEvent, DecodeError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let msg = match this.inner.poll_next(cx) {
            Poll::Ready(Some(poll)) => poll,
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };

        Poll::Ready(Some(GanCubeEvent::from_bytes(msg.as_ref())))
    }
}
