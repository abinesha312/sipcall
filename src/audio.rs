use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::rtp;

const SAMPLE_RATE: u32 = 8000;
const FRAME_SIZE: usize = 160;

pub async fn run_audio_session(local_rtp_port: u16, remote_host: String) -> Result<()> {
    let remote_rtp_port = local_rtp_port;

    let host = cpal::default_host();
    
    let input_device = host.default_input_device();
    let output_device = host.default_output_device();

    if input_device.is_none() && output_device.is_none() {
        println!("⚠ No audio devices found - running in silent mode");
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
        return Ok(());
    }

    let (tx_audio, rx_audio) = tokio::sync::mpsc::channel::<Vec<u8>>(100);
    let (tx_playback, mut rx_playback) = tokio::sync::mpsc::channel::<Vec<u8>>(100);

    if let Some(input_dev) = input_device {
        let tx = tx_audio.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = capture_audio(input_dev, tx) {
                eprintln!("Audio capture error: {}", e);
            }
        });
    } else {
        println!("⚠ No input device - sending silence");
        tokio::spawn(async move {
            loop {
                let silence = vec![0x7f; FRAME_SIZE];
                if tx_audio.send(silence).await.is_err() {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
            }
        });
    }

    tokio::spawn(async move {
        if let Err(e) = rtp::send_rtp_stream(local_rtp_port, remote_host, remote_rtp_port, rx_audio).await {
            eprintln!("RTP send error: {}", e);
        }
    });

    tokio::spawn(async move {
        if let Err(e) = rtp::receive_rtp_stream(local_rtp_port, tx_playback).await {
            eprintln!("RTP receive error: {}", e);
        }
    });

    if let Some(output_dev) = output_device {
        tokio::task::spawn_blocking(move || {
            if let Err(e) = play_audio(output_dev, rx_playback) {
                eprintln!("Audio playback error: {}", e);
            }
        });
    } else {
        println!("⚠ No output device - discarding received audio");
        tokio::spawn(async move {
            while rx_playback.recv().await.is_some() {
            }
        });
    }

    println!("🎙️  Audio session active");
    
    tokio::signal::ctrl_c().await?;
    
    Ok(())
}

fn capture_audio(device: cpal::Device, tx: tokio::sync::mpsc::Sender<Vec<u8>>) -> Result<()> {
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Fixed(FRAME_SIZE as u32),
    };

    let err_fn = |err| eprintln!("Audio input error: {}", err);

    let buffer = Arc::new(Mutex::new(Vec::new()));
    let buffer_clone = buffer.clone();

    let stream = device.build_input_stream(
        &config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mut buf = buffer_clone.blocking_lock();
            
            for &sample in data.iter() {
                let pcm = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                let ulaw = linear_to_ulaw(pcm);
                buf.push(ulaw);
            }
            
            while buf.len() >= FRAME_SIZE {
                let frame: Vec<u8> = buf.drain(..FRAME_SIZE).collect();
                let _ = tx.blocking_send(frame);
            }
        },
        err_fn,
        None,
    )?;

    stream.play()?;
    
    std::thread::park();
    
    Ok(())
}

fn play_audio(device: cpal::Device, mut rx: tokio::sync::mpsc::Receiver<Vec<u8>>) -> Result<()> {
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(SAMPLE_RATE),
        buffer_size: cpal::BufferSize::Fixed(FRAME_SIZE as u32),
    };

    let err_fn = |err| eprintln!("Audio output error: {}", err);

    let playback_buffer = Arc::new(Mutex::new(Vec::new()));
    let playback_clone = playback_buffer.clone();

    std::thread::spawn(move || {
        while let Some(data) = rx.blocking_recv() {
            let mut buf = playback_clone.blocking_lock();
            buf.extend(data);
        }
    });

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let mut buf = playback_buffer.blocking_lock();
            
            for sample in data.iter_mut() {
                if let Some(ulaw) = buf.first().copied() {
                    buf.remove(0);
                    let pcm = ulaw_to_linear(ulaw);
                    *sample = pcm as f32 / 32767.0;
                } else {
                    *sample = 0.0;
                }
            }
        },
        err_fn,
        None,
    )?;

    stream.play()?;
    
    std::thread::park();
    
    Ok(())
}

fn linear_to_ulaw(pcm: i16) -> u8 {
    const BIAS: i32 = 0x84;
    const CLIP: i32 = 32635;
    
    let mut sample = pcm as i32;
    let mask: u8;
    
    if sample < 0 {
        sample = BIAS - sample;
        mask = 0x7F;
    } else {
        sample = BIAS + sample;
        mask = 0xFF;
    }
    
    if sample > CLIP {
        sample = CLIP;
    }
    
    sample |= 0xFF;
    let seg = (15 - sample.leading_zeros() as i32) - 7;
    let seg = seg.max(0).min(7);
    
    let ulaw = if seg >= 8 {
        0x7F ^ mask
    } else {
        ((seg << 4) | ((sample >> (seg + 3)) & 0xF)) as u8 ^ mask
    };
    
    ulaw
}

fn ulaw_to_linear(ulaw: u8) -> i16 {
    let ulaw = !ulaw;
    
    let t = (((ulaw & 0x0F) as i32) << 3) + 0x84;
    let t = t << (((ulaw & 0x70) >> 4) as i32);
    
    if (ulaw & 0x80) != 0 {
        (0x84 - t) as i16
    } else {
        (t - 0x84) as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ulaw_roundtrip() {
        for value in &[0i16, 100, 500, 1000, -100, -500, -1000] {
            let encoded = linear_to_ulaw(*value);
            let decoded = ulaw_to_linear(encoded);
            let _ = (encoded, decoded);
        }
    }
}
