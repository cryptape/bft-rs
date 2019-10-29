use crate::objects::LogType;
use crate::Height;
#[allow(unused_imports)]
use log::{debug, info, log, trace, warn};
use std::collections::BTreeMap;
use std::fs::{read_dir, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::mem::transmute;
use std::str;

const DELETE_FILE_INTERVAL: u64 = 3;

pub(crate) struct Wal {
    height_fs: BTreeMap<Height, File>,
    pub(crate) dir: String,
    current_height: Height,
    ifile: File, // store off-line height
}

impl Wal {
    pub(crate) fn new(dir: &str) -> Result<Wal, io::Error> {
        let fss = read_dir(&dir);
        if fss.is_err() {
            DirBuilder::new()
                .recursive(true)
                .create(dir)
                .expect("Create wal directory failed!");
        }

        let file_path = dir.to_string() + "/" + "index";
        let expect_str = format!("Seek wal file {:?} failed!", &file_path);
        let mut ifs = OpenOptions::new()
            .read(true)
            .create(true)
            .write(true)
            .open(file_path)?;

        ifs.seek(io::SeekFrom::Start(0)).expect(&expect_str);

        let mut string_buf: String = String::new();
        let res_fsize = ifs.read_to_string(&mut string_buf)?;
        let cur_height: Height;
        let last_file_path: String;
        if res_fsize == 0 {
            last_file_path = dir.to_string() + "/0.log";
            cur_height = 0;
        } else {
            let hi_res = string_buf.parse::<Height>();
            if let Ok(hi) = hi_res {
                cur_height = hi;
                last_file_path = dir.to_string() + "/" + cur_height.to_string().as_str() + ".log"
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "index file data wrong",
                ));
            }
        }

        let fs = OpenOptions::new()
            .read(true)
            .create(true)
            .write(true)
            .open(last_file_path)?;

        let mut tmp = BTreeMap::new();
        tmp.insert(cur_height, fs);

        Ok(Wal {
            height_fs: tmp,
            dir: dir.to_string(),
            current_height: cur_height,
            ifile: ifs,
        })
    }

    fn get_file_path(dir: &str, height: Height) -> String {
        let mut name = height.to_string();
        name += ".log";
        let pathname = dir.to_string() + "/";
        pathname.clone() + &*name
    }

    pub(crate) fn set_height(&mut self, height: Height) -> Result<(), io::Error> {
        trace!("Wal set height: {:?}", height);
        self.current_height = height;
        self.ifile.seek(io::SeekFrom::Start(0))?;
        let hstr = height.to_string();
        let content = hstr.as_bytes();
        let _ = self.ifile.set_len(content.len() as u64);
        self.ifile.write_all(&content)?;
        self.ifile.sync_data()?;

        let filename = Wal::get_file_path(&self.dir, height);
        let fs = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(filename)?;
        self.height_fs.insert(height, fs);

        if height > DELETE_FILE_INTERVAL {
            let saved_height_fs = self.height_fs.split_off(&(height - DELETE_FILE_INTERVAL));
            {
                for (height, _) in self.height_fs.iter() {
                    let delfilename = Wal::get_file_path(&self.dir, *height);
                    let _ = ::std::fs::remove_file(delfilename);
                }
            }
            self.height_fs = saved_height_fs;
        }

        Ok(())
    }

    pub(crate) fn save(&mut self, height: Height, mtype: LogType, msg: &[u8]) -> io::Result<()> {
        trace!(
            "Wal save mtype: {:?}, cur_height: {}, height: {}",
            mtype,
            self.current_height,
            height
        );
        if !self.height_fs.contains_key(&height) {
            // 2 more higher than current height, do not process it
            if height > self.current_height + 1 {
                return Ok(());
            } else if height == self.current_height + 1 {
                let filename = Wal::get_file_path(&self.dir, height);
                let fs = OpenOptions::new()
                    .read(true)
                    .create(true)
                    .write(true)
                    .open(filename)?;
                self.height_fs.insert(height, fs);
            }
        }
        let mlen = msg.len() as u32;
        if mlen == 0 {
            return Ok(());
        }

        if let Some(fs) = self.height_fs.get_mut(&height) {
            let len_bytes: [u8; 4] = unsafe { transmute(mlen.to_le()) };
            let mtype: u8 = mtype.into();
            let type_bytes: [u8; 1] = unsafe { transmute(mtype.to_le()) };
            fs.seek(io::SeekFrom::End(0))?;
            fs.write_all(&len_bytes[..])?;
            fs.write_all(&type_bytes[..])?;
            fs.write_all(&msg)?;
            fs.flush()?;
        } else {
            warn!("Can't find wal log in height {} ", height);
        }

        Ok(())
    }

    pub(crate) fn load(&mut self) -> Vec<(LogType, Vec<u8>)> {
        let mut vec_buf: Vec<u8> = Vec::new();
        let mut vec_out: Vec<(LogType, Vec<u8>)> = Vec::new();
        let cur_height = self.current_height;
        info!("wal load current height {:?}", cur_height);
        info!("wal load fs {:?}", self.height_fs);
        if self.height_fs.is_empty() || cur_height == 0 {
            return vec_out;
        }

        for (height, mut fs) in &self.height_fs {
            if *height < self.current_height {
                continue;
            }
            let expect_str = format!("Seek wal file {:?} of height {} failed!", fs, *height);
            fs.seek(io::SeekFrom::Start(0)).expect(&expect_str);
            let res_fsize = fs.read_to_end(&mut vec_buf);
            if res_fsize.is_err() {
                return vec_out;
            }
            let expect_str = format!(
                "Get size of buf of wal file {:?} of height {} failed!",
                fs, *height
            );
            let fsize = res_fsize.expect(&expect_str);
            if fsize <= 5 {
                return vec_out;
            }
            let mut index = 0;
            loop {
                if index + 5 > fsize {
                    break;
                }
                let hd: [u8; 4] = [
                    vec_buf[index],
                    vec_buf[index + 1],
                    vec_buf[index + 2],
                    vec_buf[index + 3],
                ];
                let tmp: u32 = unsafe { transmute::<[u8; 4], u32>(hd) };
                let bodylen = tmp as usize;
                let mtype = vec_buf[index + 4];
                index += 5;
                if index + bodylen > fsize {
                    break;
                }
                vec_out.push((
                    LogType::from(mtype),
                    vec_buf[index..index + bodylen].to_vec(),
                ));
                index += bodylen;
            }
        }
        vec_out
    }
}
