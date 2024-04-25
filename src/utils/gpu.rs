use std::process::Command;

pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
}

pub struct GpuInfo {
    pub index: u32,
    pub name: String,
    pub memory_total: f64,
    pub memory_used: f64,
    pub memory_free: f64,
}

impl GpuInfo {
    pub fn get_gpus_info(vendor: GpuVendor) -> Vec<GpuInfo> {
        match vendor {
            GpuVendor::Nvidia => Self::get_nvidia_gpus_info(),
            _ => unimplemented!(),
        }
    }

    fn get_nvidia_gpus_info() -> Vec<GpuInfo> {
        let mut nvidia_smi = Command::new("nvidia-smi");
        nvidia_smi
            .arg("--query-gpu=index,gpu_name,memory.total,memory.free,memory.used")
            .arg("--format=csv,noheader,nounits");
        let output = nvidia_smi
            .output()
            .expect("failed to execute \"nvidia-smi\"")
            .stdout;
        let res_string = String::from_utf8(output).expect("bad output from nvidia-smi");
        let res_string = res_string.trim();
        let lines = res_string.split("\n");
        let mut cards: Vec<GpuInfo> = Vec::new();
        for card_info in lines {
            let mut card_info_items = card_info.split(",");
            let index: u32 = card_info_items
                .next()
                .unwrap()
                .parse::<u32>()
                .expect("bad info line for card");

            let name = card_info_items.next().unwrap().trim();
            let memory_total: f64 = card_info_items
                .next()
                .unwrap()
                .trim()
                .parse::<f64>()
                .expect("bad info line for card");
            let memory_free: f64 = card_info_items
                .next()
                .unwrap()
                .trim()
                .parse::<f64>()
                .expect("bad info line for card");
            let memory_used: f64 = card_info_items
                .next()
                .unwrap()
                .trim()
                .parse::<f64>()
                .expect("bad info line for card");

            cards.push(GpuInfo {
                index,
                name: name.to_string(),
                memory_total,
                memory_used,
                memory_free,
            })
        }

        cards
    }
}
