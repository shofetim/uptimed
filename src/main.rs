use std::{mem, fs, ffi::CString, net::UdpSocket, thread, time::Duration};
use libc::{self};

struct SysInfo {
    namespace: String,
    destination: String,
    interface: String,
    filesystem: String,
    hostname: String,
    last_seen_net_rx: u64,
    last_seen_net_tx: u64,
    net_rx: u64,
    net_tx: u64,
    uptime: f32,
    avail_mem: f64,
    load: f32,
    disk_free: f64,
}

impl SysInfo {
    pub fn new(
        destination: String,
        namespace: String,
        filesystem: String,
        interface: String,
    ) -> Self {
        Self {
            namespace,
            destination,
            hostname: Self::get_hostname(),
            last_seen_net_rx: Self::net_stats(&interface, "r"),
            last_seen_net_tx: Self::net_stats(&interface, "t"),
            net_rx: 0u64,
            net_tx: 0u64,
            interface,
            uptime: Self::uptime(),
            avail_mem: Self::avail_mem(),
            load: Self::load(),
            disk_free: Self::disk_free(&filesystem),
            filesystem,
        }
    }

    fn refresh(&mut self) {
        let new_net_rx = Self::net_stats(&self.interface, "r");
        let new_net_tx = Self::net_stats(&self.interface, "t");
        self.net_rx = new_net_rx - self.last_seen_net_rx;
        self.net_tx = new_net_tx - self.last_seen_net_tx;
        self.last_seen_net_rx = new_net_rx;
        self.last_seen_net_tx = new_net_tx;
        self.uptime = Self::uptime();
        self.avail_mem = Self::avail_mem();
        self.load = Self::load();
        self.disk_free = Self::disk_free(&self.filesystem);
    }

    fn get_hostname() -> String {
        let hostname =
            fs::read_to_string("/proc/sys/kernel/hostname").expect("Unable to read hostname");
        hostname.trim().to_string()
    }

    fn net_stats(interface: &str, kind: &str) -> u64 {
        fs::read_to_string(format! {"/sys/class/net/{interface}/statistics/{kind}x_bytes"})
            .expect("Unable to read statistics from provided network interface")
            .trim()
            .parse()
            .unwrap_or(0)
    }

    fn uptime() -> f32 {
        fs::read_to_string("/proc/uptime")
            .expect("Unable to read /proc/uptime")
            .trim()
            .split(" ")
            .next()
            .unwrap_or("0.0")
            .parse()
            .unwrap_or(0f32)
            .round()
    }

    fn avail_mem() -> f64 {
        let candidates: Vec<f64> = fs::read_to_string("/proc/meminfo")
            .expect("Unable to read /proc/meminfo")
            .lines()
            .filter(|l| l.starts_with("MemTotal") || l.starts_with("MemAvailable"))
            .map(|s| {
                s.split(":")
                    .last()
                    .unwrap()
                    .trim()
                    .split(" ")
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();
        let total = candidates[0];
        let avail = candidates[1];
        (avail / total * 100.0).round()
    }

    fn load() -> f32 {
        let load_avg: f32 = fs::read_to_string("/proc/loadavg")
            .expect("Unable to read /proc/loadavg")
            .trim()
            .split(" ")
            .next()
            .unwrap()
            .parse()
            .unwrap();

        let cores: f32 = fs::read_to_string("/proc/cpuinfo")
            .expect("Unable to read /proc/cpuinfo")
            .lines()
            .filter(|l| l.starts_with("processor"))
            .count()
            .to_string()
            .parse()
            .unwrap();

        (load_avg * 100f32 / cores).round()
    }

    fn disk_free(filesystem: &str) -> f64 {
        let path = CString::new(filesystem).expect("Invalid filesystem path");
        let mut stat = mem::MaybeUninit::<libc::statvfs>::uninit();
        unsafe {
            let res = libc::statvfs(path.as_ptr(), stat.as_mut_ptr());
            if res != 0 {
                println!("Cannot access filesystem stats, errno {}", res);
                return 0f64
            }
            let statvfs = stat.assume_init();
            (statvfs.f_bavail as f64 / statvfs.f_blocks as f64 * 100f64).round()
        }
    }

    /// Format metrics for statsd
    /// <https://github.com/statsd/statsd/blob/master/docs/metric_types.md>
    /// Everything we report is a gauge
    fn serialize(&self) -> String {
        let prefix = format!("{}.{}", self.namespace, self.hostname);
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n",
            format!("{}.net-rx:{}|g", prefix, self.net_rx),
            format!("{}.net-tx:{}|g", prefix, self.net_tx),
            format!("{}.uptime:{}|g", prefix, self.uptime),
            format!("{}.availmem:{}|g", prefix, self.avail_mem),
            format!("{}.diskfree:{}|g", prefix, self.disk_free),
            format!("{}.load:{}|g", prefix, self.load),
        )
    }

    fn send(&mut self) {
        self.refresh();
        let socket = UdpSocket::bind("0.0.0.0:0").expect("couldn't bind to address");
        socket
            .send_to(
                self.serialize().as_bytes(),
                format!("{}:8125", self.destination),
            )
            .expect("couldn't send data");
    }
}

fn usage() {
    println!(
        "Usage: uptimed statsd-server namespace filesystem network-interface \n\
         \n\
         Stats are pulled from the /proc filesystem \n\
         See https://www.kernel.org/doc/html/latest/filesystems/proc.html \n\
         \n\
         The following stats are emitted once per minute and sent to the StatsD host listed above\n\n\
         - hostname  /proc/sys/kernel/hostname \n\
         - net-rx    Bytes received in the last minute \n\
         - net-tx    Bytes transmitted in the last minute \n\
         - uptime    Seconds of uptime. Alert if not seen in the last 5 minutes \n\
         - availmem  Percent of memory available alert if < 20 \n\
         - diskfree  Percent of disk free alert if less than < 10 \n\
         - load      Load average, scaled 100x (to get an int) and divided by the number
            of cores. 100 is generally saturation. Alert if > 100 \n\n"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        usage();
        std::process::exit(1)
    }
    let destination = args[1].clone();
    let namespace = args[2].clone();
    let filesystem = args[3].clone();
    let interface = args[4].clone();

    let mut info = SysInfo::new(destination, namespace, filesystem, interface);

    loop {
        info.send();
        thread::sleep(Duration::from_secs(60));
    }
}
