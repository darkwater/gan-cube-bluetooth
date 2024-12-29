use std::{
    pin::Pin,
    task::{Context, Poll},
};

use aes::cipher::{Block, BlockDecryptMut as _, Iv, Key, KeyIvInit as _};
use futures::Stream;
use pin_project::pin_project;

const GAN_ENCRYPTION_KEYS: [([u8; 16], [u8; 16]); 2] = [
    (
        // Key used by GAN Gen2, Gen3 and Gen4 cubes
        [
            0x01, 0x02, 0x42, 0x28, 0x31, 0x91, 0x16, 0x07, 0x20, 0x05, 0x18, 0x54, 0x42, 0x11,
            0x12, 0x53,
        ],
        [
            0x11, 0x03, 0x32, 0x28, 0x21, 0x01, 0x76, 0x27, 0x20, 0x95, 0x78, 0x14, 0x32, 0x12,
            0x02, 0x43,
        ],
    ),
    (
        // Key used by MoYu AI 2023
        [
            0x05, 0x12, 0x02, 0x45, 0x02, 0x01, 0x29, 0x56, 0x12, 0x78, 0x12, 0x76, 0x81, 0x01,
            0x08, 0x03,
        ],
        [
            0x01, 0x44, 0x28, 0x06, 0x86, 0x21, 0x22, 0x28, 0x51, 0x05, 0x08, 0x31, 0x82, 0x02,
            0x21, 0x06,
        ],
    ),
];

pub enum CryptKey {
    /// Key used by GAN Gen2, Gen3 and Gen4 cubes
    Gan,
    /// Key used by MoYu AI 2023
    MoYu,
}

impl CryptKey {
    pub fn bytes(&self) -> (Key<Aes128Cbc>, Iv<Aes128Cbc>) {
        match self {
            CryptKey::Gan => (GAN_ENCRYPTION_KEYS[0].0.into(), GAN_ENCRYPTION_KEYS[0].1.into()),
            CryptKey::MoYu => (GAN_ENCRYPTION_KEYS[1].0.into(), GAN_ENCRYPTION_KEYS[1].1.into()),
        }
    }
}

type Aes128Cbc = cbc::Decryptor<aes::Aes128Dec>;

pub struct Decryptor {
    key: Key<Aes128Cbc>,
    iv: Iv<Aes128Cbc>,
}

impl Decryptor {
    pub fn new(key: CryptKey, mut mac_addr: [u8; 6]) -> Self {
        mac_addr.reverse();

        let (mut key, mut iv) = key.bytes();
        for n in 0..mac_addr.len() {
            key[n] = ((key[n] as u16 + mac_addr[n] as u16) % 255) as u8;
            iv[n] = ((iv[n] as u16 + mac_addr[n] as u16) % 255) as u8;
        }

        Self { key, iv }
    }

    pub fn decrypt(&self, data: &[u8]) -> Vec<u8> {
        assert!(data.len() > 16);

        let mut out = data.to_vec();
        if data.len() > 16 {
            self.decrypt_inner(&mut out, data.len() - 16);
        }
        self.decrypt_inner(&mut out, 0);

        out
    }

    fn decrypt_inner(&self, data: &mut [u8], offset: usize) {
        let data = Block::<Aes128Cbc>::from_mut_slice(data[offset..offset + 16].as_mut());
        let mut cipher = Aes128Cbc::new(&self.key, &self.iv);
        cipher.decrypt_block_mut(data);
    }
}

#[pin_project]
pub struct DecryptorStream<S: Stream<Item = T>, T: AsRef<[u8]>> {
    decryptor: Decryptor,
    #[pin]
    inner: S,
}

impl<S: Stream<Item = T>, T: AsRef<[u8]>> DecryptorStream<S, T> {
    pub fn new(inner: S, key: CryptKey, mac_addr: [u8; 6]) -> Self {
        Self {
            decryptor: Decryptor::new(key, mac_addr),
            inner,
        }
    }
}

impl<S: Stream<Item = T>, T: AsRef<[u8]>> Stream for DecryptorStream<S, T> {
    type Item = Vec<u8>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let msg = match this.inner.poll_next(cx) {
            Poll::Ready(Some(poll)) => poll,
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Pending => return Poll::Pending,
        };

        Poll::Ready(Some(this.decryptor.decrypt(msg.as_ref())))
    }
}
