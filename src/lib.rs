use derive_builder::Builder;
use logging::create_stderr_log;
use utils::PsrdadaError;
mod logging;
mod utils;
use crate::utils::PsrdadaResult;
use psrdada_sys::{
    dada_hdu_destroy, dada_hdu_t, ipcbuf_connect, ipcbuf_create, ipcbuf_destroy, ipcbuf_get_bufsz,
    ipcbuf_get_nbufs, ipcbuf_t, ipcio_connect, ipcio_create_work, ipcio_destroy, ipcio_t,
    multilog_t,
};

#[derive(Builder, Debug)]
#[builder(setter(into))]
pub struct DadaDB {
    // Non-defaultable things
    pub key: i32,
    pub log_name: String,
    #[builder(setter(skip), field(build = "self.construct()?"))]
    hdu: dada_hdu_t,
    // Defaults from psrdada
    #[builder(default = "4")]
    num_bufs: u64,
    #[builder(default = "(page_size::get() as u64) * 128")]
    buf_size: u64,
    #[builder(default = "8")]
    num_headers: u64,
    #[builder(default = "page_size::get() as u64")]
    header_size: u64,
}

impl DadaDBBuilder {
    fn construct(&self) -> PsrdadaResult<dada_hdu_t> {
        // Using all the details we collected, try to make the dada pairs, etc.
        let key = self.key.expect("Missing build arguments");
        let header_size = self.header_size.expect("Missing build arguments");
        let log_name = self.log_name.as_ref().expect("Missing build arguments");
        let num_bufs = self.num_bufs.expect("Missing build arguments");
        let buf_size = self.buf_size.expect("Missing build arguments");
        let num_headers = self.num_headers.expect("Missing build arguments");
        unsafe {
            let (mut data, mut header) = construct_hdu_pair(
                key,
                num_bufs,
                buf_size,
                num_headers,
                header_size,
                1, // I still don't know what this is for
            )?;
            // Now build the hdu
            let mut log = create_stderr_log(log_name)?;
            Ok(dada_hdu_t {
                header_block: &mut header,
                data_block: &mut data,
                header_size,
                data_block_key: key,
                header_block_key: key + 1,
                header: std::ptr::null_mut(),
                log: &mut log,
            })
        }
    }
}

unsafe fn construct_hdu_pair(
    key: i32,
    num_bufs: u64,
    buf_size: u64,
    num_headers: u64,
    header_size: u64,
    num_readers: u32,
) -> PsrdadaResult<(ipcio_t, ipcbuf_t)> {
    let mut data_ipcio = Default::default();
    let mut header_ipcbuf = Default::default();
    if ipcio_create_work(&mut data_ipcio, key, num_bufs, buf_size, num_readers, -1) < 0 {
        return Err(PsrdadaError::HDUInitError);
    }
    if ipcbuf_create(
        &mut header_ipcbuf,
        key + 1,
        num_headers,
        header_size,
        num_readers,
    ) < 0
    {
        // Cleanup the data buffer that ostensibly was allocated (unlike the source example)
        destroy_ipcio(&mut data_ipcio)?;
        return Err(PsrdadaError::HDUInitError);
    }
    Ok((data_ipcio, header_ipcbuf))
}

unsafe fn connect_ipcbuf(key: i32) -> PsrdadaResult<ipcbuf_t> {
    let mut placeholder = Default::default();
    let result = ipcbuf_connect(&mut placeholder, key);
    if result >= 0 {
        Ok(placeholder)
    } else {
        Err(PsrdadaError::HDUConnectError)
    }
}

unsafe fn connect_ipcio(key: i32) -> PsrdadaResult<ipcio_t> {
    let mut placeholder = Default::default();
    let result = ipcio_connect(&mut placeholder, key);
    if result >= 0 {
        Ok(placeholder)
    } else {
        Err(PsrdadaError::HDUConnectError)
    }
}

unsafe fn destroy_ipcbuf(buf: &mut ipcbuf_t) -> PsrdadaResult<()> {
    if ipcbuf_destroy(buf) < 0 {
        Err(PsrdadaError::HDUDestroyError)
    } else {
        Ok(())
    }
}

unsafe fn destroy_ipcio(ipc: &mut ipcio_t) -> PsrdadaResult<()> {
    if ipcio_destroy(ipc) < 0 {
        Err(PsrdadaError::HDUDestroyError)
    } else {
        Ok(())
    }
}

impl Drop for DadaDB {
    fn drop(&mut self) {
        unsafe {
            // Safety: This should be fine as hdu will be valid when it's constructed
            dada_hdu_destroy(&mut self.hdu);
        }
    }
}

impl DadaDB {
    /// Construct a [DataDB] from a preexisting buffer
    pub fn connect(key: i32, mut log: multilog_t) -> Result<Self, PsrdadaError> {
        unsafe {
            // Connect to data and header
            let mut data = connect_ipcio(key)?;
            let mut header = connect_ipcbuf(key + 1)?;

            // Grab some metadata
            let num_bufs = ipcbuf_get_nbufs(&mut data.buf);
            let buf_size = ipcbuf_get_bufsz(&mut data.buf);
            let num_headers = ipcbuf_get_nbufs(&mut header);
            let header_size = ipcbuf_get_bufsz(&mut header);

            // Construct the data_hdu
            let hdu = dada_hdu_t {
                header_block: &mut header,
                data_block: &mut data,
                header_size,
                data_block_key: key,
                header_block_key: key + 1,
                header: std::ptr::null_mut(),
                log: &mut log,
            };

            // Return
            Ok(Self {
                key,
                num_bufs,
                buf_size,
                num_headers,
                header_size,
                log_name: std::ffi::CString::from_raw(log.name)
                    .into_string()
                    .map_err(|_| PsrdadaError::FFIError)?,
                hdu,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder() {
        let db = DadaDBBuilder::default()
            .key(0xdead)
            .log_name("Test")
            .build()
            .unwrap();
        println!("{:#?}", db);
        drop(db);
    }
}
