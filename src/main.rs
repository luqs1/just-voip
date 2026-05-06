use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;

const AUDIO_PORT: u16 = 5000;
const DISCOVERY_PORT: u16 = 49999;
const PEER_TIMEOUT_SECS: u64 = 5;

#[derive(Clone)]
struct PeerInfo {
    addr: SocketAddr,
    last_seen: Instant,
    display: String,
}

struct Shared {
    peers: HashMap<String, PeerInfo>,
    muted: bool,
    volume: f32,
    incoming: HashMap<String, VecDeque<Vec<u8>>>,
}

fn main() {
    let my_id = format!("{:04x}", rand::random::<u16>());
    let shared: Arc<Mutex<Shared>> = Arc::new(Mutex::new(Shared {
        peers: HashMap::new(),
        muted: false,
        volume: 0.8,
        incoming: HashMap::new(),
    }));

    let disc_shared = shared.clone();
    let disc_id = my_id.clone();
    thread::spawn(move || discovery_loop(disc_id, disc_shared));

    let audio_shared = shared.clone();
    thread::spawn(move || audio_loop(audio_shared));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([260.0, 200.0])
            .with_resizable(false)
            .with_title("just-voip"),
        ..Default::default()
    };

    eframe::run_native(
        "just-voip",
        options,
        Box::new(|_cc| Ok(Box::new(App {
            shared,
            my_id,
            peer_ip: String::new(),
            peer_port: String::from("5000"),
        }))),
    )
    .unwrap();
}

struct App {
    shared: Arc<Mutex<Shared>>,
    my_id: String,
    peer_ip: String,
    peer_port: String,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("id: {}", self.my_id));
                ui.separator();
                let count = self.shared.lock().unwrap().peers.len();
                ui.label(format!("peers: {}", count));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("just-voip");
            ui.separator();

            let mut s = self.shared.lock().unwrap();

            ui.horizontal(|ui| {
                ui.label("Mute:");
                let was = s.muted;
                ui.toggle_value(&mut s.muted, if was { "ON" } else { "off" });
            });

            ui.horizontal(|ui| {
                ui.label("Volume:");
                ui.add(egui::Slider::new(&mut s.volume, 0.0..=1.0).text(""));
            });

            ui.separator();
            ui.label("Add peer:");

            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.peer_ip);
                ui.text_edit_singleline(&mut self.peer_port);
                if ui.button("add").clicked() {
                    if let Ok(ip) = self.peer_ip.parse::<std::net::IpAddr>() {
                        let port: u16 = self.peer_port.parse().unwrap_or(AUDIO_PORT);
                        let key = format!("{}:{}", ip, port);
                        s.peers.entry(key.clone()).or_insert_with(|| PeerInfo {
                            addr: SocketAddr::new(ip, port),
                            last_seen: Instant::now(),
                            display: key,
                        });
                    }
                    self.peer_ip.clear();
                    self.peer_port.clear();
                }
            });

            ui.separator();
            ui.label("Peers:");

            let mut stale = Vec::new();
            for (id, peer) in s.peers.iter() {
                let age = peer.last_seen.elapsed().as_secs();
                let dot = if age < 2 { "●" } else if age < PEER_TIMEOUT_SECS { "◐" } else { "○" };
                ui.horizontal(|ui| {
                    ui.label(format!("{} {}", dot, peer.display));
                    if ui.small_button("×").clicked() {
                        stale.push(id.clone());
                    }
                });
                if age >= PEER_TIMEOUT_SECS {
                    stale.push(id.clone());
                }
            }
            for id in stale {
                s.peers.remove(&id);
            }
        });

        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

fn discovery_loop(my_id: String, shared: Arc<Mutex<Shared>>) {
    let sock = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).expect("bind discovery");
    sock.set_broadcast(true).ok();
    sock.set_read_timeout(Some(Duration::from_millis(500))).ok();

    let hello = format!("HELLO {} {}", my_id, AUDIO_PORT);
    let mut last_sent = Instant::now();

    loop {
        if last_sent.elapsed() >= Duration::from_secs(2) {
            sock.send_to(hello.as_bytes(), ("255.255.255.255", DISCOVERY_PORT))
                .ok();
            last_sent = Instant::now();
        }

        let mut buf = [0u8; 256];
        if let Ok((len, addr)) = sock.recv_from(&mut buf) {
            if let Ok(msg) = std::str::from_utf8(&buf[..len]) {
                if let Some(parts) = msg.strip_prefix("HELLO ") {
                    let fields: Vec<&str> = parts.split_whitespace().collect();
                    if fields.len() == 2 {
                        let id = fields[0];
                        let port: u16 = fields[1].parse().unwrap_or(AUDIO_PORT);
                        if id != my_id {
                            let mut s = shared.lock().unwrap();
                            let key = id.to_string();
                            if let Some(p) = s.peers.get_mut(&key) {
                                p.last_seen = Instant::now();
                                p.addr = SocketAddr::new(addr.ip(), port);
                            } else {
                                s.peers.insert(key, PeerInfo {
                                    addr: SocketAddr::new(addr.ip(), port),
                                    last_seen: Instant::now(),
                                    display: format!("{}:{}", addr.ip(), port),
                                });
                            }
                        }
                    }
                }
            }
        }

        let mut s = shared.lock().unwrap();
        let stale: Vec<_> = s
            .peers
            .iter()
            .filter(|(_, v)| v.last_seen.elapsed().as_secs() >= PEER_TIMEOUT_SECS)
            .map(|(k, _)| k.clone())
            .collect();
        for k in stale {
            s.peers.remove(&k);
        }
    }
}

fn audio_loop(shared: Arc<Mutex<Shared>>) {
    let host = cpal::default_host();

    let out_device = host.default_output_device().expect("no output device");
    let out_config = out_device.default_output_config().expect("no output config");

    let in_device = host.default_input_device().expect("no input device");
    let in_config = in_device.default_input_config().expect("no input config");

    let send_sock = UdpSocket::bind(("0.0.0.0", 0)).expect("bind send");
    send_sock.set_broadcast(true).ok();
    send_sock.set_nonblocking(true).ok();
    let send_sock = Arc::new(send_sock);

    let shared_in = shared.clone();
    let shared_out = shared.clone();
    let shared_recv = shared.clone();
    let sock = send_sock.clone();

    let input_config = cpal::StreamConfig {
        sample_rate: in_config.sample_rate(),
        channels: in_config.channels(),
        buffer_size: cpal::BufferSize::Default,
    };

    let input_stream = in_device
        .build_input_stream(
            &input_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let s = shared_in.lock().unwrap();
                if s.muted || s.peers.is_empty() {
                    return;
                }
                let bytes: Vec<u8> = data
                    .iter()
                    .flat_map(|&f| ((f.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())
                    .collect();
                for (_, peer) in s.peers.iter() {
                    sock.send_to(&bytes, peer.addr).ok();
                }
            },
            move |err| eprintln!("input err: {}", err),
            None,
        )
        .expect("build input");

    let output_config = cpal::StreamConfig {
        sample_rate: out_config.sample_rate(),
        channels: out_config.channels(),
        buffer_size: cpal::BufferSize::Default,
    };

    let output_stream = out_device
        .build_output_stream(
            &output_config,
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut s = shared_out.lock().unwrap();
                let vol = s.volume;

                let samples_per_frame = output.len();
                let channels = out_config.channels() as usize;
                let mono_samples = samples_per_frame / channels;

                let mut mono_buf = vec![0i16; mono_samples];
                for out in mono_buf.iter_mut() {
                    let mut sum: i32 = 0;
                    let mut count = 0;
                    for q in s.incoming.values_mut() {
                        if !q.is_empty() {
                            if let Some(chunk) = q.front_mut() {
                                if chunk.len() >= 2 {
                                    let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as f32 * vol;
                                    sum += sample as i32;
                                    chunk.drain(..2);
                                    if chunk.is_empty() {
                                        q.pop_front();
                                    }
                                    count += 1;
                                }
                            }
                        }
                    }
                    *out = if count > 0 {
                        (sum / count as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
                    } else {
                        0
                    };
                }

                if channels == 1 {
                    for (i, &s) in mono_buf.iter().enumerate() {
                        output[i] = s as f32 / i16::MAX as f32;
                    }
                } else {
                    for (i, &s) in mono_buf.iter().enumerate() {
                        let f = s as f32 / i16::MAX as f32;
                        for ch in 0..channels {
                            output[i * channels + ch] = f;
                        }
                    }
                }
            },
            move |err| eprintln!("output err: {}", err),
            None,
        )
        .expect("build output");

    input_stream.play().expect("start input");
    output_stream.play().expect("start output");

    let recv_sock = UdpSocket::bind(("0.0.0.0", AUDIO_PORT)).expect("bind recv");
    recv_sock.set_nonblocking(true).ok();

    let mut buf = [0u8; 65535];
    loop {
        thread::sleep(Duration::from_millis(2));

        if let Ok((len, addr)) = recv_sock.recv_from(&mut buf) {
            let data = buf[..len].to_vec();
            let mut s = shared_recv.lock().unwrap();

            let mut found = None;
            for (id, peer) in s.peers.iter() {
                if peer.addr == addr {
                    found = Some(id.clone());
                    break;
                }
            }

            if found.is_none() && !s.peers.is_empty() {
                found = Some(addr.to_string());
            }

            if let Some(id) = found {
                s.incoming.entry(id).or_insert_with(VecDeque::new).push_back(data);
            }
        }

        let mut s = shared_recv.lock().unwrap();
        for q in s.incoming.values_mut() {
            while q.len() > 20 {
                q.pop_front();
            }
        }
    }
}
