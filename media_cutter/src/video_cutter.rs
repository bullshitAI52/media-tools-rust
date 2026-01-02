use anyhow::Result;
use std::process::Command;
use std::path::Path;

pub struct VideoCutter;

impl VideoCutter {
    pub fn cut_segment(input: &str, start: &str, end: &str, output: &str, reencode: bool, crf: &str, preset: &str, mute: bool) -> Result<()> {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y")
           .arg("-i").arg(input)
           .arg("-ss").arg(start)
           .arg("-to").arg(end);
           
        if mute {
            cmd.arg("-an");
        }

        if !reencode {
            cmd.arg("-c").arg("copy");
        } else {
            // Re-encode with user params
            cmd.arg("-c:v").arg("libx264")
               .arg("-crf").arg(crf)
               .arg("-preset").arg(preset);
               
            if !mute {
                cmd.arg("-c:a").arg("aac");
            }
        }

        let status = cmd.arg(output).status()?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("FFmpeg failed"))
        }
    }
    pub fn get_duration(input: &str) -> Result<f64> {
        let output = Command::new("ffprobe")
            .arg("-v").arg("error")
            .arg("-show_entries").arg("format=duration")
            .arg("-of").arg("default=noprint_wrappers=1:nokey=1")
            .arg(input)
            .output()?;
            
        let duration_str = String::from_utf8(output.stdout)?;
        let duration = duration_str.trim().parse::<f64>()?;
        Ok(duration)
    }
}
