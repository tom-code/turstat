
use clap::Parser;
use std::{fmt, time::SystemTime, collections::HashMap};
use std::io::{Read, Result, BufRead};
use byteorder::{BigEndian, ReadBytesExt};
use tokio::io::AsyncReadExt;

mod raw;

struct Packet {
    eth_dst: [u8;6],
    eth_src: [u8;6],
    eth_type: u16
}

struct Stat {
    period: u64,
    total: u64
}

impl Stat {
    fn inc(&mut self) {
        self.period = self.period + 1;
        self.total = self.total + 1;
    }
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

fn read_leases() -> Result<std::collections::HashMap<String, String>> {
    let file = std::fs::File::open("/tmp/dhcp.leases")?;
    let mut map = HashMap::new();
    for line in std::io::BufReader::new(file).lines() {
        let lineu = line?;
        let lineparts: Vec<&str> = lineu.split(" ").collect();
        if lineparts.len() >= 4 {
            map.insert(lineparts[1].to_string(), lineparts[3].to_string());
            println!("{}", lineparts[1].to_string());
        }
    }
    Ok(map)
}

fn resolve_lease(map: &std::collections::HashMap<String, String>, key: &[u8; 6]) -> String {
    let res = map.get(&mac_format(key));
    match res {
        Some(v) => v.clone(),
        None => "unknown".to_string()
    }
}

trait MapInc {
    fn inc(&mut self, key: [u8; 6]);
}
type Map = std::collections::HashMap<[u8; 6], Stat>;
impl MapInc for Map {
    fn inc(&mut self, key: [u8; 6]) {
        let count = self.entry(key).or_insert(Stat{period:0,total:0});
        count.inc();
    }
}

async fn capture_loop(device: &str, macs: &Vec<String>, period: i32) -> Result<bool>{
    let mut afdx = raw::AfPacketSocketTokio::new(device)?;
    let mut buf = vec![0u8; 2048];
    let mut map = Map::new();
    let mut time_start = SystemTime::now();
    loop {
        let _r = afdx.read(buf.as_mut()).await?;
        let parsed = parse(&buf)?;
        map.inc(parsed.eth_src);

        if time_start.elapsed().unwrap().as_secs_f32() > period as f32 {
            let leases = read_leases()?;
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
                    let table_column = [mac_format(i.0), resolve_lease(&leases, i.0), i.1.total.to_string(), i.1.period.to_string()];
                    table_builder.push_record(table_column);
                    i.1.period = 0;
                }
                table_builder.insert_record(0, ["src", "resolved", "total", "period"]);
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
