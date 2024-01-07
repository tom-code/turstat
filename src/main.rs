
use clap::Parser;
use libc::{
    socket, ETH_P_ALL, SOCK_RAW,
};
pub use libc::{AF_PACKET, IFF_PROMISC, PF_PACKET};

use std::{io::Error, io::{Result, ErrorKind}, pin::Pin, task::{Poll, Context}, fmt, time::SystemTime};
use tokio::io::{unix::AsyncFd, ReadBuf, AsyncRead};
//use futures_lite::ready;
use std::os::unix::prelude::{AsRawFd,RawFd};
use std::io::Read;
use byteorder::{BigEndian, ReadBytesExt};



use tokio::io::AsyncReadExt;


#[derive(Debug)]
struct FDX {
    fd: RawFd
}

#[derive(Debug)]
struct AFDX {
    afd: AsyncFd<FDX>
}


fn ifindex_by_name(name: &str) -> Result<i32> {
    if name.len() >= libc::IFNAMSIZ {
        return Err(ErrorKind::InvalidInput.into());
    }
    let mut buf = [0u8; libc::IFNAMSIZ];
    buf[..name.len()].copy_from_slice(name.as_bytes());
    let idx = unsafe { libc::if_nametoindex(buf.as_ptr() as *const libc::c_char) };
    if idx == 0 {
        return Err(Error::last_os_error());
    }
    Ok(idx as i32)
}


impl FDX {
    fn new(device: &str) -> Result<Self> {

        // create socket
        let fd = unsafe { socket(AF_PACKET, SOCK_RAW, (ETH_P_ALL as u16).to_be() as i32) };
        if fd == -1 {
            return Err(Error::last_os_error());
        }


        // bind interface
        let ifindex = ifindex_by_name(device)?;
        unsafe {
            let mut sll: libc::sockaddr_ll = std::mem::zeroed();
            sll.sll_family = AF_PACKET as u16;
            sll.sll_protocol = (ETH_P_ALL as u16).to_be();
            sll.sll_ifindex = ifindex;

            let sa = &sll as *const libc::sockaddr_ll as *const libc::sockaddr;
            let res = libc::bind(fd, sa, std::mem::size_of::<libc::sockaddr_ll>() as u32);
            if res == -1 {
                return Err(Error::last_os_error());
            }
        }

    
        //set non-blocking
        let mut res = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if res != -1 {
            res = unsafe { libc::fcntl(fd, libc::F_SETFL, res | libc::O_NONBLOCK) };
        }
        if res == -1 {
            return Err(Error::last_os_error());
        }

        return Ok(Self{fd})
    }
}

impl AsRawFd for FDX {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}
impl Read for FDX {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        read_fd(self.fd, buf)
    }
}
impl<'a> Read for &'a FDX {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        read_fd(self.fd, buf)
    }
}
impl Drop for FDX {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

impl AsyncRead for AFDX {
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

impl<'a> AsyncRead for &'a AFDX {
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


impl AFDX {
    fn new(device: &str) -> Result<Self> {
        let fdx = FDX::new(device)?;
        let afd = AsyncFd::new(fdx)?;
        Ok(Self{afd})
    }
}

fn read_fd(fd: RawFd, buf: &mut [u8]) -> Result<usize> {
    let rv = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
    if rv < 0 {
        return Err(Error::last_os_error());
    }

    Ok(rv as usize)
}

struct Packet {
    eth_dst: [u8;6],
    eth_src: [u8;6],
    eth_type: u16
}

struct Stat {
    period: u64,
    total: u64
}


fn mac_format(i: &[u8]) -> String {
    format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", i[0], i[1], i[2], i[3], i[4], i[5])
}

impl fmt::Debug for Packet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Packet")
         .field("eth.src", &mac_format(&self.eth_src))
         .field("eth.dst", &mac_format(&self.eth_dst))
         .field("eth.typ", &format!("0x{:x}", self.eth_type))
         .finish()
    }
}

fn parse(data : &Vec<u8>) -> Result<Packet>{
    let mut cursor = std::io::Cursor::new(data);
    let mut eth_src: [u8; 6] = [0;6];
    let mut eth_dst: [u8; 6] = [0;6];
    Read::read(&mut cursor, &mut eth_dst)?;
    Read::read(&mut cursor, &mut eth_src)?;
    let eth_type = ReadBytesExt::read_u16::<BigEndian>(&mut cursor)?;
    Ok(Packet{
        eth_dst,
        eth_src,
        eth_type
    })
}

async fn capture_loop(device: &str, macs: &Vec<String>, period: i32) -> Result<bool>{
    let mut afdx = AFDX::new(device).unwrap();
    let mut buf = vec![0u8; 2048];
    let mut map = std::collections::HashMap::new();
    let mut time_start = SystemTime::now();
    loop {
        let _r = afdx.read(buf.as_mut()).await?;
        let parsed = parse(&buf)?;
        let count = map.entry(parsed.eth_src).or_insert(Stat{period:0,total:0});
        count.period += 1;
        count.total += 1;

        if time_start.elapsed().unwrap().as_secs_f32() > period as f32 {
            time_start = SystemTime::now();
            let datetime : chrono::DateTime<chrono::Local> = time_start.into();
            if macs.len() == 1 {
                let count = map.entry(parsed.eth_src).or_insert(Stat{period:0,total:0});
                println!("{} {}", datetime.to_rfc2822(), count.period);
                count.period = 0;
            } else {
                println!("\n{}", datetime.to_rfc2822());
                let mut table_builder = tabled::builder::Builder::default();
                for i in &mut map {
                    let table_column = [mac_format(i.0), i.1.total.to_string(), i.1.period.to_string()];
                    table_builder.push_record(table_column);
                    i.1.period = 0;
                }
                table_builder.insert_record(0, ["src", "total", "period"]);
                let mut table = table_builder.build();
                table.with(tabled::settings::Style::modern());
                println!("{}", table.to_string())
            }
        }
    }
}



#[derive(clap::Parser)]
#[command()] 
struct Args {
    device: String,

    #[arg(short, long)]
    mac: Vec<String>,

    #[arg(short, long)]
    period: i32,
}      

fn main() {
    let args = Args::parse();
    println!("{:?}", args.mac);

    let runtime = tokio::runtime::Runtime::new().unwrap();
    println!("prepare to capture [device:{}]", args.device);
    runtime.block_on( async  {
        capture_loop(&args.device, &args.mac, args.period).await.unwrap()
    });
}
