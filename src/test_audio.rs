use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const AUDIO_PORT: u16 = 5005;

fn main() {
    let mut passed = 0;
    let mut failed = 0;

    println!("=== just-voip tests ===\n");

    // Test 1: Open input device
    print!("[1] Open input device... ");
    let host = cpal::default_host();
    let in_device = host.default_input_device().expect("no input device");
    let in_config = in_device.default_input_config().expect("no input config");
    println!("OK ({})", in_device.name().unwrap_or("?".into()));
    passed += 1;

    // Test 2: Open output device
    print!("[2] Open output device... ");
    let out_device = host.default_output_device().expect("no output device");
    let out_config = out_device.default_output_config().expect("no output config");
    println!("OK ({})", out_device.name().unwrap_or("?".into()));
    passed += 1;

    // Test 3: Capture audio for 1 second
    print!("[3] Capture 1s of audio... ");
    let config = cpal::StreamConfig {
        sample_rate: in_config.sample_rate(),
        channels: in_config.channels(),
        buffer_size: cpal::BufferSize::Default,
    };

    let captured: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let cap = captured.clone();
    let done = Arc::new(Mutex::new(false));
    let done_clone = done.clone();

    let stream = in_device
        .build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let bytes: Vec<u8> = data
                    .iter()
                    .flat_map(|&f| ((f.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())
                    .collect();
                cap.lock().unwrap().extend_from_slice(&bytes);
            },
            move |_| {},
            None,
        )
        .expect("build input");
    stream.play().expect("start input");

    thread::sleep(Duration::from_secs(1));
    *done_clone.lock().unwrap() = true;
    drop(stream);

    let data = captured.lock().unwrap().clone();
    if data.len() > 100 {
        println!("OK ({} bytes captured)", data.len());
        passed += 1;
    } else {
        println!("FAIL (only {} bytes — mic might be muted)", data.len());
        failed += 1;
    }

    // Test 4: Play back captured audio
    print!("[4] Playback captured audio... ");
    let out_config = cpal::StreamConfig {
        sample_rate: out_config.sample_rate(),
        channels: out_config.channels(),
        buffer_size: cpal::BufferSize::Default,
    };

    let playback_data = data.clone();
    let pos = Arc::new(Mutex::new(0usize));
    let pos_clone = pos.clone();

    let out_stream = out_device
        .build_output_stream(
            &out_config,
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut p = pos_clone.lock().unwrap();
                let channels = out_config.channels as usize;
                for frame in output.chunks_mut(channels) {
                    let sample = if *p < playback_data.len() - 1 {
                        let s = i16::from_le_bytes([playback_data[*p], playback_data[*p + 1]]) as f32 / i16::MAX as f32;
                        *p += 2;
                        s
                    } else {
                        0.0
                    };
                    for ch in frame.iter_mut() {
                        *ch = sample;
                    }
                }
            },
            move |_| {},
            None,
        )
        .expect("build output");
    out_stream.play().expect("start output");

    let samples_to_play = data.len() / 2;
    let duration_ms = (samples_to_play as f64 / in_config.sample_rate().0 as f64 * 1000.0) as u64;
    thread::sleep(Duration::from_millis(duration_ms + 200));
    drop(out_stream);

    println!("OK (played {} samples)", samples_to_play);
    passed += 1;

    // Test 5: UDP send/receive
    print!("[5] UDP send/receive on port {}... ", AUDIO_PORT);
    let tx = UdpSocket::bind(("127.0.0.1", 0)).expect("bind tx");
    let rx = UdpSocket::bind(("127.0.0.1", AUDIO_PORT)).expect("bind rx");
    rx.set_read_timeout(Some(Duration::from_secs(2))).ok();

    tx.send_to(&data[..64], ("127.0.0.1", AUDIO_PORT))
        .expect("send");

    let mut recv_buf = [0u8; 4096];
    let (len, _) = rx.recv_from(&mut recv_buf).expect("recv");
    if recv_buf[..len] == data[..64] {
        println!("OK");
        passed += 1;
    } else {
        println!("FAIL (data mismatch)");
        failed += 1;
    }

    println!("\n=== {} passed, {} failed ===", passed, failed);

    if failed > 0 {
        std::process::exit(1);
    }
}
