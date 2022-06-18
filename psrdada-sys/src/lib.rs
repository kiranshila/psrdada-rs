#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

// Include generated bindings
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[cfg(test)]
mod handwriten_tests {
    use super::*;
    use page_size;

    unsafe fn default_ipcbuf() -> ipcbuf_t {
        ipcbuf_t {
            state: 0,
            syncid: -1,
            semid_connect: -1,
            semid_data: 0 as *mut _,
            shmid: 0 as *mut _,
            sync: 0 as *mut _,
            buffer: 0 as *mut _,
            shm_addr: 0 as *mut _,
            count: 0 as *mut _,
            shmkey: 0 as *mut _,
            viewbuf: 0,
            xfer: 0,
            soclock_buf: 0,
            iread: -1,
        }
    }

    #[test]
    fn test_create_and_destroy() {
        // Arguments
        let key = 0xdead;
        let num_bufs = 4;
        let buf_size = (page_size::get() as u64) * 128;
        let num_headers = 8;
        let header_size = page_size::get() as u64;
        let num_readers = 1;
        let device_id = -1; // No CUDA
        unsafe {
            let mut header_block = default_ipcbuf();
            let mut data_block = default_ipcbuf();
            assert!(
                ipcbuf_create_work(
                    &mut data_block,
                    key,
                    num_bufs,
                    buf_size,
                    num_readers,
                    device_id,
                ) >= 0
            );
            assert!(
                ipcbuf_create(
                    &mut header_block,
                    key + 1,
                    num_headers,
                    header_size,
                    num_readers,
                ) >= 0
            );
            // Now try locking in memory
            assert!(ipcbuf_lock(&mut data_block) >= 0);
            assert!(ipcbuf_lock(&mut header_block) >= 0);
            // Cleanup
            ipcbuf_destroy(&mut data_block);
            ipcbuf_destroy(&mut header_block);
        }
    }
}
