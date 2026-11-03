use super::FRAME;

pub(super) struct Encoder(*mut opus_head_sys::OpusEncoder);
impl Encoder {
    pub(super) fn new() -> Result<Self, String> {
        let mut err = 0;
        let p = unsafe {
            opus_head_sys::opus_encoder_create(
                16000,
                1,
                opus_head_sys::OPUS_APPLICATION_AUDIO as i32,
                &mut err,
            )
        };
        if p.is_null() || err != 0 {
            Err("Could not start audio encoder".into())
        } else {
            Ok(Self(p))
        }
    }
    pub(super) fn encode(&mut self, frame: &[f32]) -> Result<Vec<u8>, String> {
        let mut out = vec![0; 4000];
        let n = unsafe {
            opus_head_sys::opus_encode_float(
                self.0,
                frame.as_ptr(),
                FRAME as i32,
                out.as_mut_ptr(),
                out.len() as i32,
            )
        };
        if n < 0 {
            return Err("Could not encode recording".into());
        }
        out.truncate(n as usize);
        Ok(out)
    }
}
impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe { opus_head_sys::opus_encoder_destroy(self.0) }
    }
}
pub(super) struct Decoder(*mut opus_head_sys::OpusDecoder, pub(super) u32);
impl Decoder {
    pub(super) fn new(rate: u32) -> Result<Self, String> {
        let mut err = 0;
        let p = unsafe { opus_head_sys::opus_decoder_create(rate as i32, 1, &mut err) };
        if p.is_null() || err != 0 {
            Err("Could not start audio decoder".into())
        } else {
            Ok(Self(p, rate))
        }
    }
    pub(super) fn decode(&mut self, packet: &[u8]) -> Result<Vec<f32>, String> {
        let mut out = vec![0.; self.1 as usize * 120 / 1000];
        let n = unsafe {
            opus_head_sys::opus_decode_float(
                self.0,
                packet.as_ptr(),
                packet.len() as i32,
                out.as_mut_ptr(),
                out.len() as i32,
                0,
            )
        };
        if n < 0 {
            return Err("Could not decode assistant audio".into());
        }
        out.truncate(n as usize);
        Ok(out)
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { opus_head_sys::opus_decoder_destroy(self.0) }
    }
}
