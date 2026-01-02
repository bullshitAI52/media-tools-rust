use eframe::egui;
use rfd::FileDialog;
use std::fs;
use regex::Regex;
use std::path::{Path, PathBuf};
use tokio::runtime::Runtime;

mod video_cutter;

use common::ai::{DeepSeekClient, Segment};
use video_cutter::VideoCutter;

struct MediaCutterApp {
    input_path: String,
    output_dir: String,
    segments: Vec<Segment>,
    
    // DeepSeek
    deepseek_key: String,
    deepseek_prompt: String,
    
    // Status
    log: String,
    reencode_enabled: bool,
    mute_enabled: bool,
    
    enc_crf: String,
    enc_preset: String,
    
    // Quick Trim
    trim_head: String,
    trim_tail: String,
    
    // Auto Split
    split_count: String,
    split_duration: String,
    
    // Naming
    output_template: String,
    
    // Runtime
    rt: Runtime,
}

impl Default for MediaCutterApp {
    fn default() -> Self {
        Self {
            input_path: String::new(),
            output_dir: std::env::current_dir().unwrap().display().to_string(),
            segments: vec![],
            deepseek_key: std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
            deepseek_prompt: "提取精彩片段".to_owned(),
            log: "就绪。".to_owned(),
            reencode_enabled: false,
            mute_enabled: false,
            enc_crf: "23".to_owned(),
            enc_preset: "medium".to_owned(),
            trim_head: "0".to_owned(),
            trim_tail: "0".to_owned(),
            split_count: "3".to_owned(),
            split_duration: "10".to_owned(),
            output_template: "segment_{}".to_owned(),
            rt: Runtime::new().unwrap(),
        }
    }
}

impl MediaCutterApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_custom_fonts(&cc.egui_ctx);
        Self::default()
    }
    
    fn log(&mut self, msg: &str) {
        self.log = format!("{}\n{}", self.log, msg);
    }
}

impl eframe::App for MediaCutterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Drag & Drop
        if !ctx.input(|i| i.raw.dropped_files.is_empty()) {
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            if let Some(file) = dropped.first() {
                if let Some(path) = &file.path {
                     self.input_path = path.display().to_string();
                     self.log(&format!("已为您加载文件: {}", self.input_path));
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("媒体剪辑工具 (Rust 版)");
            ui.separator();

            ui.collapsing("📝 使用说明 / Usage Instructions", |ui| {
                ui.label(egui::RichText::new("模式一：修剪 (直接执行)").strong());
                ui.label("   - **快速去头去尾**: 输入秒数 -> 点击“⚡ 执行” -> 立即保存文件。");
                ui.add_space(5.0);
                
                ui.label(egui::RichText::new("模式二：分段 (先生成，后执行)").strong());
                ui.label("   - **AI 分析 / 自动均分**: 点击按钮后，片段会先显示在上方列表中。");
                ui.label("   - **确认无误**: 检查列表后，点击底部的“🚀 开始剪辑”批量保存。");
                ui.add_space(5.0);
                
                ui.label(egui::RichText::new("通用设置").strong());
                ui.label("   - **导入 SRT**: 加载字幕文件作为剪辑点 (也属于模式二)。");
                ui.label("   - **精准切割**: 勾选后会重新编码 (慢但精确)，支持 CRF/Preset 设置；不勾选则流复制 (极速)。");
            });
            ui.separator();

            // File Selection
            egui::Grid::new("file_grid").num_columns(3).show(ui, |ui| {
                ui.label("输入文件:");
                ui.text_edit_singleline(&mut self.input_path);
                if ui.button("浏览...").clicked() {
                    if let Some(path) = FileDialog::new().pick_file() {
                        self.input_path = path.display().to_string();
                    }
                }
                ui.end_row();

                ui.label("输出目录:");
                ui.text_edit_singleline(&mut self.output_dir);
                if ui.button("浏览...").clicked() {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        self.output_dir = path.display().to_string();
                    }
                }
                ui.end_row();
            });

            ui.separator();
            
            // DeepSeek Panel
            ui.collapsing("AI 分析 (DeepSeek)", |ui| {
                ui.horizontal(|ui| {
                    ui.label("API 密钥:");
                    ui.add(egui::TextEdit::singleline(&mut self.deepseek_key).password(true));
                });
                ui.horizontal(|ui| {
                    ui.label("提示词:");
                    ui.text_edit_singleline(&mut self.deepseek_prompt);
                });
                if ui.button("分析视频").clicked() {
                     let key = self.deepseek_key.clone();
                     let prompt = self.deepseek_prompt.clone();
                     
                     self.log("开始分析...");
                     
                     let client = DeepSeekClient::new(key);
                     if let Ok(segs) = self.rt.block_on(client.analyze_segments(&prompt, "placeholder content")) {
                         self.segments = segs;
                         self.log("分析完成。");
                     }
                }
            });

            ui.separator();
            
            // Segments Table
            ui.horizontal(|ui| {
                ui.label("剪辑片段:");
                if ui.button("添加行").clicked() {
                    self.segments.push(Segment {
                        start: "".to_owned(), end: "".to_owned(), text: "".to_owned()
                    });
                }
                if ui.button("清空").clicked() {
                    self.segments.clear();
                }
                if ui.button("📂 导入 SRT").clicked() {
                     if let Some(path) = FileDialog::new().add_filter("SRT/Text", &["srt", "txt"]).pick_file() {
                         if let Ok(content) = fs::read_to_string(&path) {
                             let re = Regex::new(r"(?m)^\d+\s+(\d{2}:\d{2}:\d{2},\d{3})\s+-->\s+(\d{2}:\d{2}:\d{2},\d{3})\s+((?:.|\n)*?)(?:\r?\n\r?\n|$)").unwrap();
                             self.segments.clear();
                             for caps in re.captures_iter(&content) {
                                 if let (Some(start), Some(end), Some(text)) = (caps.get(1), caps.get(2), caps.get(3)) {
                                     self.segments.push(Segment {
                                         start: start.as_str().replace(',', "."),
                                         end: end.as_str().replace(',', "."),
                                         text: text.as_str().replace('\n', " ").trim().to_string(), 
                                     });
                                 }
                             }
                             self.log(&format!("从 SRT 导入了 {} 个片段。", self.segments.len()));
                         } else {
                             self.log("无法读取 SRT 文件。");
                         }
                     }
                }
            });

            egui::ScrollArea::vertical()
                .id_source("segments_scroll")
                .max_height(300.0)
                .show(ui, |ui| {
                egui::Grid::new("segments_grid").striped(true).show(ui, |ui| {
                    ui.label("#");
                    ui.label("开始时间");
                    ui.label("结束时间");
                    ui.label("描述内容");
                    ui.label("操作");
                    ui.end_row();

                    let mut to_remove = None;
                    for (i, seg) in self.segments.iter_mut().enumerate() {
                        ui.label((i + 1).to_string());
                        ui.text_edit_singleline(&mut seg.start);
                        ui.text_edit_singleline(&mut seg.end);
                        ui.text_edit_singleline(&mut seg.text);
                        if ui.button("X").clicked() {
                            to_remove = Some(i);
                        }
                        ui.end_row();
                    }
                    if let Some(i) = to_remove {
                        self.segments.remove(i);
                    }
                });
            });

            ui.separator();
            
            // Quick Trim
            ui.heading("✂️ 快速去头去尾 / Quick Trim");
            ui.horizontal(|ui| {
                ui.label("去头 (秒):");
                ui.text_edit_singleline(&mut self.trim_head).request_focus();
                
                ui.label("去尾 (秒):");
                ui.text_edit_singleline(&mut self.trim_tail);
                
                if ui.button("⚡ 执行去头去尾").clicked() {
                     let input = self.input_path.clone();
                     let output_dir = self.output_dir.clone();
                     let head_s: f64 = self.trim_head.parse().unwrap_or(0.0);
                     let tail_s: f64 = self.trim_tail.parse().unwrap_or(0.0);
                     let reencode = self.reencode_enabled;
                     let mute = self.mute_enabled;
                     let crf = self.enc_crf.clone();
                     let preset = self.enc_preset.clone();
                     
                     if input.is_empty() {
                         self.log("请先选择输入文件。");
                         return;
                     }
                     
                     self.log("正在计算时长...");
                     
                     // In a real app, do this async
                     match VideoCutter::get_duration(&input) {
                         Ok(duration) => {
                             self.log(&format!("视频总时长: {:.2} 秒", duration));
                             let start = head_s;
                             let end = duration - tail_s;
                             
                             if start >= end {
                                 self.log("错误: 去头去尾后时长无效 (Start >= End)");
                             } else {
                                 let start_str = common::time_utils::seconds_to_time_str(start).replace(',', ".");
                                 let end_str = common::time_utils::seconds_to_time_str(end).replace(',', ".");
                                 
                                 let output_name = format!("{}/trimmed_output.mp4", output_dir);
                                 self.log(&format!("剪辑范围: {} -> {}", start_str, end_str));
                                 
                                 match VideoCutter::cut_segment(&input, &start_str, &end_str, &output_name, reencode, &crf, &preset, mute) {
                                     Ok(_) => self.log(&format!("✅ 剪辑完成: {}", output_name)),
                                     Err(e) => self.log(&format!("❌ 剪辑失败: {}", e)),
                                 }
                             }
                         }
                         Err(e) => self.log(&format!("无法获取时长 (需要 ffprobe): {}", e)),
                     }
                }
            });

            ui.separator();
            
            // Auto Split
            ui.heading("📏 自动均分 / Auto Split");
            ui.horizontal(|ui| {
                ui.label("按段数均分:");
                ui.add(egui::TextEdit::singleline(&mut self.split_count).desired_width(50.0));
                if ui.button("生成 N 段").clicked() {
                    let input = self.input_path.clone();
                    let count_res = self.split_count.parse::<usize>();
                    
                    if input.is_empty() {
                         self.log("请先选择输入文件。");
                    } else if let Ok(n) = count_res {
                         if n == 0 {
                             self.log("段数必须大于 0");
                         } else {
                             match VideoCutter::get_duration(&input) {
                                 Ok(duration) => {
                                     self.segments.clear();
                                     let chunk_len = duration / (n as f64);
                                     for i in 0..n {
                                         let start = i as f64 * chunk_len;
                                         let end = if i == n - 1 { duration } else { (i + 1) as f64 * chunk_len };
                                         
                                         self.segments.push(Segment {
                                             start: common::time_utils::seconds_to_time_str(start).replace(',', "."),
                                             end: common::time_utils::seconds_to_time_str(end).replace(',', "."),
                                             text: format!("Part {}/{}", i + 1, n),
                                         });
                                     }
                                     self.log(&format!("已生成 {} 个均分片段，请检查上方列表。", n));
                                 }
                                 Err(e) => self.log(&format!("无法获取时长: {}", e)),
                             }
                         }
                    } else {
                        self.log("请输入有效的段数。");
                    }
                }
                
                ui.separator();
                
                ui.label("按时长均分 (分):");
                ui.add(egui::TextEdit::singleline(&mut self.split_duration).desired_width(50.0));
                if ui.button("每 N 分钟一段").clicked() {
                    let input = self.input_path.clone();
                    let dur_res = self.split_duration.parse::<f64>();
                    
                    if input.is_empty() {
                         self.log("请先选择输入文件。");
                    } else if let Ok(minutes) = dur_res {
                         if minutes <= 0.0 {
                             self.log("时长必须大于 0");
                         } else {
                             match VideoCutter::get_duration(&input) {
                                 Ok(duration) => {
                                     self.segments.clear();
                                     let chunk_len = minutes * 60.0;
                                     let mut start = 0.0;
                                     let mut i = 1;
                                     
                                     while start < duration {
                                         let end = (start + chunk_len).min(duration);
                                         self.segments.push(Segment {
                                             start: common::time_utils::seconds_to_time_str(start).replace(',', "."),
                                             end: common::time_utils::seconds_to_time_str(end).replace(',', "."),
                                             text: format!("Part {} ({}m)", i, minutes),
                                         });
                                         start = end;
                                         if start >= duration - 0.1 { break; } // Avoid tiny last fragment
                                         i += 1;
                                     }
                                     self.log(&format!("已生成 {} 个固定时长片段，请检查上方列表。", self.segments.len()));
                                 }
                                 Err(e) => self.log(&format!("无法获取时长: {}", e)),
                             }
                         }
                    } else {
                         self.log("请输入有效的时长 (分钟)。");
                    }
                }
            });

            ui.separator();

            // Actions
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.reencode_enabled, "精准切割 (重新编码)");
                ui.checkbox(&mut self.mute_enabled, "🔇 去除音频");
                
                if self.reencode_enabled {
                    ui.label("CRF:");
                    ui.add(egui::TextEdit::singleline(&mut self.enc_crf).desired_width(30.0));
                    ui.label("Preset:");
                    egui::ComboBox::from_id_salt("preset_combo")
                        .selected_text(&self.enc_preset)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.enc_preset, "ultrafast".to_string(), "Ultrafast");
                            ui.selectable_value(&mut self.enc_preset, "superfast".to_string(), "Superfast");
                            ui.selectable_value(&mut self.enc_preset, "veryfast".to_string(), "Veryfast");
                            ui.selectable_value(&mut self.enc_preset, "faster".to_string(), "Faster");
                            ui.selectable_value(&mut self.enc_preset, "fast".to_string(), "Fast");
                            ui.selectable_value(&mut self.enc_preset, "medium".to_string(), "Medium");
                            ui.selectable_value(&mut self.enc_preset, "slow".to_string(), "Slow");
                        });
                }
                
                ui.separator();
                ui.label("命名模板:");
                ui.add(egui::TextEdit::singleline(&mut self.output_template).desired_width(120.0))
                    .on_hover_text("使用 {} 代表序号。例如: my_video_{}");

                if ui.button("🚀 开始剪辑").clicked() {
                     self.log("开始剪辑...");
                     let mut logs = Vec::new();
                     let crf = self.enc_crf.clone();
                     let preset = self.enc_preset.clone();
                     let template = self.output_template.clone();
                     let mute = self.mute_enabled;
                     
                     for (i, seg) in self.segments.iter().enumerate() {
                         let filename = if template.contains("{}") {
                             template.replace("{}", &(i + 1).to_string())
                         } else {
                             format!("{}_{}", template, i + 1)
                         };
                         let out_name = format!("{}/{}.mp4", self.output_dir, filename);
                         
                         match VideoCutter::cut_segment(
                             &self.input_path, 
                             &seg.start, 
                             &seg.end, 
                             &out_name, 
                             self.reencode_enabled,
                             &crf,
                             &preset,
                             mute
                         ) {
                             Ok(_) => logs.push(format!("片段 {} 已保存。", i)),
                             Err(e) => logs.push(format!("片段 {} 错误: {}", i, e)),
                         }
                     }
                     for msg in logs {
                         self.log(&msg);
                     }
                     self.log("全部完成。");
                }
            });
            
            ui.separator();
            ui.label("运行日志:");
            egui::ScrollArea::vertical().id_source("logs_scroll").show(ui, |ui| {
                ui.monospace(&self.log);
            });
        });
    }
}

fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    
    let font_candidates = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ];
    
    let mut font_data = None;
    for path in font_candidates {
        if let Ok(data) = std::fs::read(path) {
            font_data = Some(data);
            break;
        }
    }
    
    if let Some(data) = font_data {
        fonts.font_data.insert(
            "my_font".to_owned(),
            egui::FontData::from_owned(data).tweak(
                egui::FontTweak {
                    scale: 1.2, 
                    ..Default::default()
                }
            ),
        );
        
        fonts.families.entry(egui::FontFamily::Proportional).or_default()
            .insert(0, "my_font".to_owned());
        fonts.families.entry(egui::FontFamily::Monospace).or_default()
            .insert(0, "my_font".to_owned());
            
        ctx.set_fonts(fonts);
    }
}

fn main() -> eframe::Result<()> {
    env_logger::init();
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Media Cutter",
        native_options,
        Box::new(|cc| Ok(Box::new(MediaCutterApp::new(cc)))),
    )
}
