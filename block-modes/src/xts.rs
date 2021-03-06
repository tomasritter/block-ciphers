use block_cipher_trait::BlockCipher;
use block_cipher_trait::generic_array::typenum::Unsigned;
use block_cipher_trait::generic_array::GenericArray;
use block_padding::Padding;
use traits::BlockMode;
use utils::{xor, Block, to_blocks, to_blocks_uneven, swap, get_next_tweak};
use core::marker::PhantomData;
use std::clone::Clone;
use errors::{InvalidKeyIvLength, BlockModeError};
use std::vec::Vec;

/// Xor encrypt xor with ciphertext stealing (XTS) block cipher mode instance.
///
/// Note that `new` method ignores IV, so during initialization you can
/// just pass `Default::default()` instead.
///
/// [1]: https://en.wikipedia.org/wiki/Block_cipher_mode_of_operation#XTS
pub struct Xts<C: BlockCipher, P: Padding> {
    cipher: C,
    tweak: GenericArray<u8, C::BlockSize>,
    _p: PhantomData<P>,
}

impl<C: BlockCipher, P: Padding> BlockMode<C, P> for Xts<C, P> {
    // If new is used to create the cipher, _iv already needs to be encrypted
    // by the second key so it can be used as a tweak value
    fn new(cipher: C, _iv: &Block<C>) -> Self {
        assert_eq!(C::BlockSize::to_usize(), 128 / 8); // Only block ciphers with 128 bit block size
        Self {
            cipher,
            tweak: _iv.clone(),
            _p: Default::default()
        }
    }

    fn new_var(key: &[u8], _iv: &[u8]) -> Result<Self, InvalidKeyIvLength> {
        assert_eq!(C::BlockSize::to_usize(), 128 / 8); // Only block ciphers with 128 bit block size
        if key.len() != C::KeySize::to_usize() * 2 || _iv.len() != C::BlockSize::to_usize() {
            return Err(InvalidKeyIvLength)
        }

        let cipher = C::new_varkey(&key[..C::KeySize::to_usize()]).map_err(|_| InvalidKeyIvLength)?;
        let tweak_cipher = C::new_varkey(&key[C::KeySize::to_usize()..]).map_err(|_| InvalidKeyIvLength)?;
        let mut tweak : GenericArray<u8, C::BlockSize> = Default::default();
        tweak[..C::BlockSize::to_usize()].copy_from_slice(_iv);
        tweak_cipher.encrypt_block(&mut tweak);

        Ok(
            Self {
            cipher,
            tweak,
            _p: Default::default()
            }
        )
    }

    fn encrypt_blocks(&mut self, blocks: &mut [Block<C>]) {
        for block in blocks {
            xor(block, &self.tweak);
            self.cipher.encrypt_block(block);
            xor(block, &self.tweak);
            get_next_tweak(&mut self.tweak);
        }
    }

    fn decrypt_blocks(&mut self, blocks: &mut [Block<C>]) {
        for block in blocks {
            xor(block, &self.tweak);
            self.cipher.decrypt_block(block);
            xor(block, &self.tweak);
            get_next_tweak(&mut self.tweak);
        }
    }

    /// Encrypt message in-place.
    ///
    /// pos argument is ignored, since padding is not used with XTS.
    fn encrypt(
        mut self, buffer: &mut [u8], _: usize
    ) -> Result<&[u8], BlockModeError> {
        let bs = C::BlockSize::to_usize();
        let buffer_length = buffer.len();
        self.encrypt_blocks(to_blocks_uneven(buffer));
        if buffer_length % bs != 0 {
            let encrypted_len = (buffer_length / bs) * bs;
            let leftover = buffer_length - encrypted_len;
            let last_block_index = buffer_length - bs;
            assert!(buffer_length - last_block_index == bs);
            let mut last_block = &mut to_blocks(&mut buffer[last_block_index..])[0];
            swap(&mut last_block, bs - leftover);
            xor(&mut last_block, &self.tweak);
            self.cipher.encrypt_block(&mut last_block);
            xor(&mut last_block, &self.tweak);
            swap(&mut buffer[buffer_length - leftover - bs..], leftover);
        }
        Ok(buffer)
    }

    /// Decrypt message in-place.
    fn decrypt(mut self, buffer: &mut [u8]) -> Result<&[u8], BlockModeError> {
        let bs = C::BlockSize::to_usize();
        let buffer_length = buffer.len();
        let num_of_full_blocks = buffer_length / bs;
        self.decrypt_blocks(&mut to_blocks_uneven(buffer)[..&num_of_full_blocks - 1]);

        if buffer_length % bs != 0 {
            let second_to_last_tweak = self.tweak.clone();
            get_next_tweak(&mut self.tweak);
            let leftover = buffer_length - (buffer_length / bs) * bs;

            {
                let mut last_block = &mut to_blocks_uneven(buffer)[num_of_full_blocks - 1];
                xor(&mut last_block, &self.tweak);
                self.cipher.decrypt_block(&mut last_block);
                xor(&mut last_block, &self.tweak);
            }

            swap(&mut buffer[buffer_length - leftover - bs..], bs);
            swap(&mut buffer[buffer_length - bs..], leftover);
            self.tweak = second_to_last_tweak;
        }
        let last_block = &mut to_blocks_uneven(buffer)[num_of_full_blocks - 1];
        xor(last_block, &self.tweak);
        self.cipher.decrypt_block(last_block);
        xor(last_block, &self.tweak);
        Ok(buffer)
    }

    /// Encrypt message and store result in vector.
    #[cfg(feature = "std")]
    fn encrypt_vec(self, plaintext: &[u8]) -> Vec<u8> {
        let mut buf = Vec::from(plaintext);
        match self.encrypt(&mut buf, 0) { // 0 is ig
            Ok(_) => buf,
            _ => panic!()
        }
    }

    /// Encrypt message and store result in vector.
    #[cfg(feature = "std")]
    fn decrypt_vec(self, ciphertext: &[u8]) -> Result<Vec<u8>, BlockModeError> {
        let mut buf = Vec::from(ciphertext);
        match self.decrypt(&mut buf) {
            Ok(_) => Ok(buf),
            Err(e) => Err(e)
        }
    }
}
