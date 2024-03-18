#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

// Include generated bindings
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

// We need to include some default constructors, as those #defines don't work in bindgen
impl Default for ipcbuf_t {
    fn default() -> Self {
        Self {
            state: Default::default(),
            syncid: -1,
            semid_connect: -1,
            semid_data: std::ptr::null_mut(),
            shmid: std::ptr::null_mut(),
            sync: std::ptr::null_mut(),
            buffer: std::ptr::null_mut(),
            shm_addr: std::ptr::null_mut(),
            count: std::ptr::null_mut(),
            shmkey: std::ptr::null_mut(),
            viewbuf: Default::default(),
            xfer: Default::default(),
            soclock_buf: Default::default(),
            iread: -1,
        }
    }
}

impl Default for ipcio_t {
    fn default() -> Self {
        Self {
            buf: Default::default(),
            curbuf: std::ptr::null_mut(),
            curbufsz: Default::default(),
            bytes: Default::default(),
            rdwrt: Default::default(),
            marked_filled: Default::default(),
            sod_pending: Default::default(),
            sod_buf: Default::default(),
            sod_byte: Default::default(),
            buf_ptrs: std::ptr::null_mut(),
            bufs_opened: Default::default(),
            bufs_opened_max: Default::default(),
        }
    }
}

impl Default for dada_hdu_t {
    fn default() -> Self {
        Self {
            log: std::ptr::null_mut(),
            data_block: std::ptr::null_mut(),
            header_block: std::ptr::null_mut(),
            header: std::ptr::null_mut(),
            header_size: Default::default(),
            data_block_key: Default::default(),
            header_block_key: Default::default(),
        }
    }
}
