use std::{pin::Pin, ffi::CString};
use std::task::{Poll, Context};
use std::os::unix::prelude::{AsRawFd,RawFd};
use std::io::{Read, Result, ErrorKind, Error};
use tokio::io::{unix::AsyncFd, ReadBuf, AsyncRead};



#[derive(Debug)]
struct AfPacketSocket {
    fd: RawFd
}

#[derive(Debug)]
pub struct AfPacketSocketTokio {
    afd: AsyncFd<AfPacketSocket>
}


fn ifindex_by_name(name: &str) -> Result<u32> {
    if name.len() >= libc::IFNAMSIZ {
        return Err(ErrorKind::InvalidInput.into());
    }
    let namecstr = CString::new(name)?;
    let idx = unsafe { libc::if_nametoindex(namecstr.as_ptr()) };
    if idx == 0 {
        return Err(Error::last_os_error());
    }
    Ok(idx)
}


impl AfPacketSocket {
    fn new(device: &str) -> Result<Self> {
        // create socket
        let fd = unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW, (libc::ETH_P_ALL as u16).to_be() as i32) };
        if fd == -1 {
            return Err(Error::last_os_error());
        }

        // bind interface
        let ifindex = ifindex_by_name(device)?;
        unsafe {
            let mut sll: libc::sockaddr_ll = std::mem::zeroed();
            sll.sll_family = libc::AF_PACKET as u16;
            sll.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
            sll.sll_ifindex = ifindex as i32;

            let sa = &sll as *const libc::sockaddr_ll as *const libc::sockaddr;
            let res = libc::bind(fd, sa, std::mem::size_of::<libc::sockaddr_ll>() as u32);
            if res == -1 {
                return Err(Error::last_os_error());
            }
        }

        //set non-blocking
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags == -1 {
            return Err(Error::last_os_error()); 
        }
        let res = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if res == -1 {
            return Err(Error::last_os_error());
        }

        return Ok(Self{fd})
    }
}

impl AsRawFd for AfPacketSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

fn read_fd(fd: RawFd, buf: &mut [u8]) -> Result<usize> {
    let rv = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    if rv < 0 {
        return Err(Error::last_os_error());
    }

    Ok(rv as usize)
}

impl Read for AfPacketSocket {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        read_fd(self.fd, buf)
    }
}
impl<'a> Read for &'a AfPacketSocket {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        read_fd(self.fd, buf)
    }
}
impl Drop for AfPacketSocket {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

impl AsyncRead for AfPacketSocketTokio {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf,
    ) -> Poll<Result<()>> {
        loop {
            //let mut guard = ready!(self.afd.poll_read_ready(cx))?;
            let mut guard = match self.afd.poll_read_ready(cx) {
                core::task::Poll::Ready(t) => t,
                core::task::Poll::Pending => return core::task::Poll::Pending,
            }?;

            match guard.try_io(|inner| inner.get_ref().read(buf.initialize_unfilled())) {
                Ok(result) => {
                    buf.advance(result?);
                    return Poll::Ready(Ok(()));
                },
                Err(_would_block) => continue,
            }
        }
    }
}

impl<'a> AsyncRead for &'a AfPacketSocketTokio {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf,
    ) -> Poll<Result<()>> {
        loop {
            //let mut guard = ready!(self.afd.poll_read_ready(cx))?;
            let mut guard = match self.afd.poll_read_ready(cx) {
                core::task::Poll::Ready(t) => t,
                core::task::Poll::Pending => return core::task::Poll::Pending,
            }?;

            match guard.try_io(|inner| inner.get_ref().read(buf.initialize_unfilled())) {
                Ok(result) => {
                    buf.advance(result?);
                    return Poll::Ready(Ok(()));
                },
                Err(_would_block) => continue,
            }
        }
    }
}


impl AfPacketSocketTokio {
    pub fn new(device: &str) -> Result<Self> {
        let fdx = AfPacketSocket::new(device)?;
        let afd = AsyncFd::new(fdx)?;
        Ok(Self{afd})
    }
}
