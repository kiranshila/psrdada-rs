use psrdada_sys::ipcbuf_get_bufsz;
use psrdada_sys::ipcbuf_get_nbufs;
use psrdada_sys::{
    ipcbuf_connect, ipcbuf_create, ipcbuf_create_work, ipcbuf_destroy, ipcbuf_lock, ipcbuf_t,
};

#[derive(Debug)]
pub enum PsrdadaError {
    HDUInitError,
    HDUConnectError,
    HDUDestroyError,
}

#[derive(Debug)]
pub struct DadaDB {
    pub key: i32,
    num_bufs: u64,
    buf_size: u64,
    num_headers: u64,
    header_size: u64,
    // ipcbuf_t instances
    header_ipcbuf: ipcbuf_t,
    data_ipcbuf: ipcbuf_t,
}

unsafe fn default_ipcbuf() -> ipcbuf_t {
    ipcbuf_t {
        state: 0,
        syncid: -1,
        semid_connect: -1,
        semid_data: std::ptr::null_mut(),
        shmid: std::ptr::null_mut(),
        sync: std::ptr::null_mut(),
        buffer: std::ptr::null_mut(),
        shm_addr: std::ptr::null_mut(),
        count: std::ptr::null_mut(),
        shmkey: std::ptr::null_mut(),
        viewbuf: 0,
        xfer: 0,
        soclock_buf: 0,
        iread: -1,
    }
}

unsafe fn construct_hdu_pair(
    key: i32,
    num_bufs: u64,
    buf_size: u64,
    num_headers: u64,
    header_size: u64,
    num_readers: u32,
) -> Result<(ipcbuf_t, ipcbuf_t), PsrdadaError> {
    let mut data_ipcbuf = default_ipcbuf();
    let mut header_ipcbuf = default_ipcbuf();
    if ipcbuf_create_work(&mut data_ipcbuf, key, num_bufs, buf_size, num_readers, -1) < 0 {
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
        destroy_hdu(&mut data_ipcbuf)?;
        return Err(PsrdadaError::HDUInitError);
    }
    Ok((data_ipcbuf, header_ipcbuf))
}

unsafe fn connect_hdu(key: i32) -> Result<ipcbuf_t, PsrdadaError> {
    let mut placeholder = default_ipcbuf();
    let result = ipcbuf_connect(&mut placeholder, key);
    if result >= 0 {
        Ok(placeholder)
    } else {
        Err(PsrdadaError::HDUConnectError)
    }
}

unsafe fn destroy_hdu(buf: &mut ipcbuf_t) -> Result<(), PsrdadaError> {
    if ipcbuf_destroy(buf) < 0 {
        Err(PsrdadaError::HDUDestroyError)
    } else {
        Ok(())
    }
}

impl DadaDB {
    pub fn new(
        key: i32,
        num_bufs: Option<u64>,
        buf_size: Option<u64>,
        num_headers: Option<u64>,
        header_size: Option<u64>,
    ) -> Result<Self, PsrdadaError> {
        let num_bufs = num_bufs.unwrap_or(4);
        let buf_size = buf_size.unwrap_or((page_size::get() as u64) * 128);
        let num_headers = num_headers.unwrap_or(8);
        let header_size = header_size.unwrap_or(page_size::get() as u64);

        // Try to actually construct
        unsafe {
            let (data_buf, header_buf) =
                construct_hdu_pair(key, num_bufs, buf_size, num_headers, header_size, 1)?;
            Ok(Self {
                key,
                num_bufs,
                buf_size,
                num_headers,
                header_size,
                header_ipcbuf: header_buf,
                data_ipcbuf: data_buf,
            })
        }
    }

    /// Construct a [DataDB] from a preexisting buffer
    pub fn connect(key: i32) -> Result<Self, PsrdadaError> {
        unsafe {
            let mut data_ipcbuf = connect_hdu(key)?;
            let mut header_ipcbuf = connect_hdu(key + 1)?;
            Ok(Self {
                key,
                num_bufs: ipcbuf_get_nbufs(&mut data_ipcbuf),
                buf_size: ipcbuf_get_bufsz(&mut data_ipcbuf),
                num_headers: ipcbuf_get_nbufs(&mut header_ipcbuf),
                header_size: ipcbuf_get_bufsz(&mut header_ipcbuf),
                header_ipcbuf,
                data_ipcbuf,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name() {
        let foo = DadaDB::connect(0xdead).unwrap();
        dbg!(foo);
        panic!()
    }
}
