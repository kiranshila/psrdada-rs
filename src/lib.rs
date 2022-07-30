mod builder;
mod client;
mod errors;
mod io;
#[cfg(test)]
mod tests;

// #[derive(Debug)]
// pub struct ReadHalf<'db> {
//     parent: &'db DadaDB,
//     holding_page: bool,
// }

// #[derive(Debug)]
// pub struct WriteHalf<'db> {
//     parent: &'db DadaDB,
// }

// impl DadaDB {
//     /// Splits the DadaDB into the read and write pairs
//     pub fn split(&self) -> (ReadHalf, WriteHalf) {
//         (
//             ReadHalf {
//                 parent: self,
//                 holding_page: false,
//             },
//             WriteHalf { parent: self },
//         )
//     }
// }

// impl<'db> WriteHalf<'db> {
//     /// Clear all the state of the writer
//     pub fn reset(&mut self) -> PsrdadaResult<()> {
//         unsafe {
//             // Safety: HDU is valid for the lifetime of Self, we're checking NULL explicitly
//             // Lock the writer
//             if dada_hdu_lock_write(self.parent.hdu) != 0 {
//                 return Err(PsrdadaError::HDULockingError);
//             }
//             let mut data_buf = (*(*self.parent.hdu).data_block).buf;
//             // Reset
//             if ipcbuf_reset(&mut data_buf) != 0 {
//                 return Err(PsrdadaError::HDUEODError);
//             }
//             // Unlock the writer
//             if dada_hdu_unlock_write(self.parent.hdu) != 0 {
//                 return Err(PsrdadaError::HDULockingError);
//             }
//         }
//         Ok(())
//     }
// }

// #[gat]
// impl<'db> LendingIterator for ReadHalf<'db> {
//     type Item<'next> = PsrdadaResult<&'next [u8]>;

//     fn next(&'_ mut self) -> Option<Self::Item<'_>> {
//         unsafe {
//             // Lock the reader
//             if dada_hdu_lock_read(self.parent.hdu) != 0 {
//                 return Some(Err(PsrdadaError::HDULockingError));
//             }
//             let mut data_buf = (*(*self.parent.hdu).data_block).buf;
//             // If we had data already, clear the previous page
//             if self.holding_page {
//                 if ipcbuf_mark_cleared(&mut data_buf) != 0 {
//                     return Some(Err(PsrdadaError::HDUReadError));
//                 }
//                 self.holding_page = false;
//             }
//             // Check if we're EOD
//             if ipcbuf_eod(&mut data_buf) == 1 {
//                 if ipcbuf_reset(&mut data_buf) != 0 {
//                     return Some(Err(PsrdadaError::HDUResetError));
//                 }
//                 // Make sure we unlock before we return (I think)
//                 if dada_hdu_unlock_read(self.parent.hdu) != 0 {
//                     return Some(Err(PsrdadaError::HDULockingError));
//                 }
//                 None
//             } else {
//                 // If not, grab the next stuff
//                 let mut bytes_available = 0u64;
//                 // Safety: HDU is valid for the lifetime of Self, we're checking NULL explicitly
//                 let data_ptr =
//                     ipcbuf_get_next_read(&mut data_buf, &mut bytes_available) as *const u8;
//                 // Check against null
//                 if data_ptr.is_null() {
//                     return Some(Err(PsrdadaError::HDUReadError));
//                 }
//                 self.holding_page = true;
//                 // Construct our lifetime tracked thing
//                 let raw_slice = std::slice::from_raw_parts(data_ptr, bytes_available as usize);
//                 // Unlock the reader
//                 if dada_hdu_unlock_read(self.parent.hdu) != 0 {
//                     return Some(Err(PsrdadaError::HDULockingError));
//                 }
//                 Some(Ok(raw_slice))
//             }
//         }
//     }
// }

// #[gat]
// impl<'db> LendingIterator for WriteHalf<'db> {
//     type Item<'next> = PsrdadaResult<&'next mut [u8]>;

//     fn next(&'_ mut self) -> Option<Self::Item<'_>> {
//         unsafe {
//             // Lock the reader
//             if dada_hdu_lock_write(self.parent.hdu) != 0 {
//                 return Some(Err(PsrdadaError::HDULockingError));
//             }
//             let mut data_buf = (*self.parent.hdu).data_block;
//             // Safety: HDU is valid for the lifetime of Self, we're checking NULL explicitly
//             let data_ptr = ipcio_open_block_write(data_buf, 0); // wtf is block_idx
//             if data_ptr.is_null() {
//                 return Some(Err(PsrdadaError::HDUReadError));
//             }
//             // Create raw slice
//             let raw_slice = std::slice::from_raw_parts_mut(data_ptr, self.parent.buf_size as usize);
//             // Unlock the reader
//             if dada_hdu_unlock_write(self.parent.hdu) != 0 {
//                 return Some(Err(PsrdadaError::HDULockingError));
//             }
//             Some(Ok(raw_slice))
//         }
//     }
// }

// // Headers

// impl<'db> WriteHalf<'db> {
//     /// Push a `header` map onto the ring buffer
//     /// Blocking, this will wait until there is a header page available
//     pub fn push_header(&self, header: &HashMap<String, String>) -> PsrdadaResult<()> {
//         unsafe {
//             // Lock the writer
//             if dada_hdu_lock_write(self.parent.hdu) != 0 {
//                 return Err(PsrdadaError::HDULockingError);
//             }
//             let header_buf = (*self.parent.hdu).header_block;
//             // Read the next ptr
//             let header_ptr = ipcbuf_get_next_write(header_buf) as *mut u8;
//             // Check against null
//             if header_ptr.is_null() {
//                 return Err(PsrdadaError::HDUWriteError);
//             }
//             // Grab the region we'll "print" into
//             let slice = std::slice::from_raw_parts_mut(
//                 header_ptr as *mut u8,
//                 self.parent.header_size as usize,
//             );
//             let header_str: String = Itertools::intersperse(
//                 header.iter().map(|(k, v)| format!("{} {}", k, v)),
//                 "\n".to_owned(),
//             )
//             .collect();
//             let header_bytes = header_str.into_bytes();
//             if header_bytes.len() > slice.len() {
//                 return Err(PsrdadaError::HeaderOverflow);
//             }
//             // Memcpy
//             slice[0..header_bytes.len()].clone_from_slice(&header_bytes);
//             // Tell PSRDada we're done
//             if ipcbuf_mark_filled(header_buf, header_bytes.len() as u64) != 0 {
//                 return Err(PsrdadaError::HDUWriteError);
//             }
//             // Unlock the writer
//             if dada_hdu_unlock_write(self.parent.hdu) != 0 {
//                 return Err(PsrdadaError::HDULockingError);
//             }
//         }
//         Ok(())
//     }
// }

// impl<'db> ReadHalf<'db> {
//     /// Blocking read the next header from the buffer and return as a hashmap
//     /// Each pair is separated by whitespace (tab or space) and pairs are separated with newlines
//     pub fn next_header(&self) -> PsrdadaResult<HashMap<String, String>> {
//         unsafe {
//             // Lock the reader
//             if dada_hdu_lock_read(self.parent.hdu) != 0 {
//                 return Err(PsrdadaError::HDULockingError);
//             }

//             let header_buf = (*self.parent.hdu).header_block;
//             let mut bytes_available = 0u64;
//             let header_ptr = ipcbuf_get_next_read(header_buf, &mut bytes_available) as *mut u8;
//             // Check if we got nothing
//             if bytes_available == 0 {
//                 if ipcbuf_mark_cleared(header_buf) != 0 {
//                     return Err(PsrdadaError::HDUReadError);
//                 }
//                 if ipcbuf_eod(header_buf) == 1 {
//                     if ipcbuf_reset(header_buf) != 0 {
//                         return Err(PsrdadaError::HDUResetError);
//                     }
//                 }
//             }
//             // Grab the header
//             let header_str = std::str::from_utf8(std::slice::from_raw_parts(
//                 header_ptr as *const u8,
//                 bytes_available as usize,
//             ))
//             .map_err(|_| PsrdadaError::UTF8Error)?;
//             let dict = match header_str
//                 .lines()
//                 .map(|s| {
//                     let mut split = s.split_ascii_whitespace();
//                     let key = split.next().to_owned();
//                     let value = split.next().to_owned();
//                     if let Some(key) = key {
//                         if let Some(value) = value {
//                             Some((key.to_owned(), value.to_owned()))
//                         } else {
//                             None
//                         }
//                     } else {
//                         None
//                     }
//                 })
//                 .collect()
//             {
//                 Some(d) => d,
//                 None => return Err(PsrdadaError::UTF8Error),
//             };
//             // Mark as read
//             if ipcbuf_mark_cleared(header_buf) != 0 {
//                 return Err(PsrdadaError::HDUReadError);
//             }
//             Ok(dict)
//         }
//     }
// }

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_read_write() {
//         let key = 8;
//         let my_hdu = DadaDBBuilder::new(key, "read_write")
//             .buf_size(5)
//             .build()
//             .unwrap();
//         // Push some bytes
//         let (mut reader, mut writer) = my_hdu.split();
//         let bytes = [0u8, 2u8, 3u8, 4u8, 5u8];
//         writer.push(&bytes).unwrap();
//         let page = reader.next().unwrap().unwrap();
//         assert_eq!(bytes, page);
//     }

//     #[test]
//     fn test_multi_read_write() {
//         let key = 10;
//         let my_hdu = DadaDBBuilder::new(key, "read_write")
//             .buf_size(4)
//             .build()
//             .unwrap();
//         let (mut reader, mut writer) = my_hdu.split();
//         // Push some bytes
//         let bytes = [0u8, 2u8, 3u8, 4u8];
//         writer.push(&bytes).unwrap();
//         let bytes_two = [10u8, 11u8, 12u8, 13u8];
//         writer.push(&bytes_two).unwrap();
//         // Use an explicit scope so Rust knows the borrow is valid
//         let first_page = reader.next().unwrap().unwrap();
//         assert_eq!(bytes, first_page);
//         let second_page = reader.next().unwrap().unwrap();
//         assert_eq!(bytes_two, second_page);
//     }

//     #[test]
//     fn test_multithread_read_write() {
//         let key = 12;
//         let my_hdu = DadaDBBuilder::new(key, "read_write").build().unwrap();
//         let (_, mut writer) = my_hdu.split();
//         // Push some bytes
//         let bytes = [0u8, 2u8, 3u8, 4u8, 5u8];
//         writer.push(&bytes).unwrap();
//         // Spawn the thread, but wait for the read to finish before destroying
//         std::thread::spawn(move || {
//             let my_hdu = DadaDB::connect(key, "Another HDU log").unwrap();
//             let mut reader = my_hdu.split().0;
//             let page = reader.next().unwrap().unwrap();
//             assert_eq!(bytes, page);
//         })
//         .join()
//         .unwrap();
//     }

//     #[test]
//     #[should_panic]
//     fn test_too_much_data() {
//         let key = 14;
//         let my_hdu = DadaDBBuilder::new(key, "read_write")
//             .buf_size(2) // Limit buffer to 2
//             .build()
//             .unwrap();
//         let (mut reader, mut writer) = my_hdu.split();
//         writer.push(&[0u8; 3]).unwrap();
//         let page = reader.next().unwrap().unwrap();
//         assert_eq!([0u8; 3], page);
//     }

//     #[test]
//     fn test_eod_and_reset() {
//         let key = 16;
//         let my_hdu = DadaDBBuilder::new(key, "test").build().unwrap();
//         // Push some bytes
//         let (mut reader, mut writer) = my_hdu.split();
//         // Writing 5 bytes will EOD as it's less than the buffer size
//         let bytes = [0u8, 2u8, 3u8, 4u8, 5u8];
//         writer.push(&bytes).unwrap();
//         let page = reader.next().unwrap().unwrap();
//         assert_eq!(bytes, page);
//         assert_eq!(None, reader.next());
//         // The None reset the buffer
//         let bytes_next = [42u8, 124u8];
//         writer.push(&bytes_next).unwrap();
//         let page = reader.next().unwrap().unwrap();
//         assert_eq!(bytes_next, page);
//     }

//     #[test]
//     fn test_sizing_shmem() {
//         let key = 18;
//         let my_hdu = DadaDBBuilder::new(key, "An HDU log")
//             .num_bufs(1)
//             .buf_size(128)
//             .num_headers(4)
//             .header_size(64)
//             .lock(true)
//             .build()
//             .unwrap();
//         assert_eq!(my_hdu.buf_size, 128);
//         assert_eq!(my_hdu.header_size, 64);
//     }

//     #[test]
//     fn test_read_write_header() {
//         let key = 20;
//         let my_hdu = DadaDBBuilder::new(key, "test").build().unwrap();
//         let header = HashMap::from([
//             ("START_FREQ".to_owned(), "1530".to_owned()),
//             ("STOP_FREQ".to_owned(), "1280".to_owned()),
//             ("TSAMP".to_owned(), "8.193e-6".to_owned()),
//         ]);
//         let (reader, writer) = my_hdu.split();
//         writer.push_header(&header).unwrap();
//         let header_read = reader.next_header().unwrap();
//         for (k, v) in header_read.into_iter() {
//             assert_eq!(&header.get(&k).unwrap(), &v.as_str());
//         }
//     }
// }
