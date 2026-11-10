use std::env;
use std::fs::OpenOptions;
use std::fs;
use std::path::PathBuf;
use std::io::{self, Read, Write};
use std::process::{Command, Stdio};
use std::os::unix::process::CommandExt;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::randr::{ConnectionExt as RandrExt, GetMonitorsReply};
use x11rb::protocol::xfixes::ConnectionExt as XfixesExt;
use x11rb::protocol::xproto::{ConnectionExt as XprotoExt, ImageFormat, Window};
use x11rb::rust_connection::RustConnection;

const VID: u16 = 0x1a86;
const PID: u16 = 0xfe07;
const HID_REPORT: usize = 65;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb {
}

#[derive(Clone, Copy)]
struct Frame<'a> {
    width: usize,
    height: usize,
    rgba: &'a [u8],
}

impl<'a> Frame<'a> {
    #[inline]
    fn pixel(&self, x: usize, y: usize) -> Rgb {
        let i = (y * self.width + x) * 4;
        Rgb { b: self.rgba[i], g: self.rgba[i + 1], r: self.rgba[i + 2] }
    }
}

#[inline]
fn clamp_u8(v: u32) -> u8 { v.min(255) as u8 }

/// Fast box average. It samples a narrow strip at the edge and does not resize
/// the desktop image. This is the default low-latency estimator.
fn box_edge(frame: Frame<'_>, x0: usize, y0: usize, w: usize, h: usize) -> Rgb {
    let mut r = 0u64;
    let mut g = 0u64;
    let mut b = 0u64;
    let mut n = 0u64;
    let x1 = (x0 + w).min(frame.width);
    let y1 = (y0 + h).min(frame.height);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = frame.pixel(x, y);
            r += p.r as u64;
            g += p.g as u64;
            b += p.b as u64;
            n += 1;
        }
    }
    if n == 0 { return Rgb::default(); }
    Rgb { r: (r / n) as u8, g: (g / n) as u8, b: (b / n) as u8 }
}

/// A 3x3 Gaussian kernel. It preserves bright edges better than a plain mean,
/// but intentionally costs more; benchmark decides when it is worthwhile.
fn gaussian3(frame: Frame<'_>, cx: usize, cy: usize) -> Rgb {
    const K: [[u32; 3]; 3] = [[1, 2, 1], [2, 4, 2], [1, 2, 1]];
    let mut r = 0u32;
    let mut g = 0u32;
    let mut b = 0u32;
    let mut weight = 0u32;
    for (ky, row) in K.iter().enumerate() {
        let y = (cy + ky).saturating_sub(1).min(frame.height - 1);
        for (kx, &w) in row.iter().enumerate() {
            let x = (cx + kx).saturating_sub(1).min(frame.width - 1);
            let p = frame.pixel(x, y);
            r += p.r as u32 * w;
            g += p.g as u32 * w;
            b += p.b as u32 * w;
            weight += w;
        }
    }
    Rgb { r: clamp_u8(r / weight), g: clamp_u8(g / weight), b: clamp_u8(b / weight) }
}

/// Luma-weighted estimator. It is useful for game/movie modes where a few
/// saturated UI pixels should not dominate an entire LED segment.
fn luma_weighted(frame: Frame<'_>, x0: usize, y0: usize, w: usize, h: usize) -> Rgb {
    let mut r = 0u64;
    let mut g = 0u64;
    let mut b = 0u64;
    let mut weight = 0u64;
    let x1 = (x0 + w).min(frame.width);
    let y1 = (y0 + h).min(frame.height);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = frame.pixel(x, y);
            let lum = (54 * p.r as u64 + 183 * p.g as u64 + 19 * p.b as u64) / 256;
            let weight_i = 32 + lum;
            r += p.r as u64 * weight_i;
            g += p.g as u64 * weight_i;
            b += p.b as u64 * weight_i;
            weight += weight_i;
        }
    }
    if weight == 0 { return Rgb::default(); }
    Rgb { r: (r / weight) as u8, g: (g / weight) as u8, b: (b / weight) as u8 }
}

#[inline]
fn checksum(bytes: &[u8]) -> u8 { bytes.iter().fold(0u8, |sum, &v| sum.wrapping_add(v)) }

fn rb_frame(id: &mut u8, action: u8, payload: &[u8]) -> Vec<u8> {
    let size = 6 + payload.len();
    assert!(size <= 255, "RB frame exceeds protocol length field");
    let mut frame = Vec::with_capacity(size);
    frame.extend_from_slice(b"RB");
    frame.push(size as u8);
    frame.push(*id);
    frame.push(action);
    frame.extend_from_slice(payload);
    frame.push(checksum(&frame));
    *id = id.wrapping_add(1).max(2);
    frame
}

fn hid_report(frame: &[u8]) -> [u8; HID_REPORT] {
    let mut report = [0u8; HID_REPORT];
    report[1..1 + frame.len()].copy_from_slice(frame);
    report
}

fn write_rb_frame<W: Write>(device: &mut W, frame: &[u8]) -> io::Result<()> {
    for chunk in frame.chunks(64) {
        let mut report = [0u8; HID_REPORT];
        report[1..1 + chunk.len()].copy_from_slice(chunk);
        device.write_all(&report)?;
    }
    Ok(())
}

fn probe(path: &str) -> io::Result<()> {
    let mut device = OpenOptions::new().read(true).write(true).open(path)?;
    let mut id = 2;
    let frame = rb_frame(&mut id, 130, &[]);
    let report = hid_report(&frame);
    device.write_all(&report)?;
    device.flush()?;
    println!("probe write: {path} interface 0, VID:PID {VID:04x}:{PID:04x}, {} bytes", report.len());
    Ok(())
}

fn set_color(path: &str, rgb: Rgb) -> io::Result<()> {
    let mut device = OpenOptions::new().write(true).open(path)?;
    let mut id = 2;
    // Broadcast the selected color from LED 1 through the final LED.
    let frame = rb_frame(&mut id, 134, &[1, rgb.r, rgb.g, rgb.b, 254]);
    let report = hid_report(&frame);
    device.write_all(&report)?;
    device.flush()?;
    println!("sent solid color #{:02X}{:02X}{:02X} to {path}", rgb.r, rgb.g, rgb.b);
    Ok(())
}

fn screen_frame(id: &mut u8, colors: &[Rgb]) -> Vec<u8> {
    // Native screen-sync command carries all LEDs in one SC/128 message.
    // Device protocol supports 16-bit frame length and HID slices of 64 bytes.
    let total=7+colors.len()*5;
    let mut frame=Vec::with_capacity(total);
    frame.extend_from_slice(b"SC");
    frame.extend_from_slice(&(total as u16).to_be_bytes());
    frame.extend_from_slice(&[*id,128]);
    for (i,c) in colors.iter().enumerate(){let n=(i+1) as u8;frame.extend_from_slice(&[n,c.r,c.g,c.b,n]);}
    frame.push(checksum(&frame));
    *id = id.wrapping_add(1);
    if *id == 0 || *id == 255 { *id = 1; }
    frame
}

fn send_led_colors(device: &mut impl Write, id: &mut u8, colors: &[Rgb]) -> io::Result<()> {
    let frame = screen_frame(id, colors);
    write_rb_frame(device, &frame)?;
    device.flush()
}

#[derive(Clone, Copy, Debug)]
struct Monitor { x: i16, y: i16, w: u16, h: u16 }

fn monitors(conn: &RustConnection, root: Window) -> Result<Vec<Monitor>, Box<dyn std::error::Error>> {
    let reply: GetMonitorsReply = conn.randr_get_monitors(root, true)?.reply()?;
    Ok(reply.monitors.into_iter().map(|m| Monitor { x: m.x, y: m.y, w: m.width, h: m.height }).collect())
}

/// Measures X11 capture of only the three LED coverage bands. This is a
/// production diagnostic: it reports the alternative before selecting it.
fn bench_roi_capture(monitor_index: usize) -> Result<(), Box<dyn std::error::Error>> {
    let (conn, screen_num)=x11rb::connect(None)?;
    let root=conn.setup().roots[screen_num].root;
    let m=*monitors(&conn,root)?.get(monitor_index).ok_or("monitor index is out of range")?;
    let band_w=(m.w/31).max(1);let band_h=(m.h/17).max(1);
    let left_x=(m.x as i32+m.w as i32*2/100) as i16;
    let right_x=(m.x as i32+m.w as i32-m.w as i32*2/100-band_w as i32) as i16;
    let top_y=(m.y as i32+m.h as i32*2/100) as i16;
    let bytes_per_frame=(band_w as usize*m.h as usize*2+m.w as usize*band_h as usize)*4;
    let mut times=Vec::with_capacity(240);let start=Instant::now();
    for _ in 0..240 {
        let t=Instant::now();
        let left=conn.get_image(ImageFormat::Z_PIXMAP,root,left_x,m.y,band_w,m.h,u32::MAX)?;
        let top=conn.get_image(ImageFormat::Z_PIXMAP,root,m.x,top_y,m.w,band_h,u32::MAX)?;
        let right=conn.get_image(ImageFormat::Z_PIXMAP,root,right_x,m.y,band_w,m.h,u32::MAX)?;
        let a=left.reply()?;let b=top.reply()?;let c=right.reply()?;
        std::hint::black_box((a.data.len(),b.data.len(),c.data.len()));
        times.push(t.elapsed().as_secs_f64()*1000.0);
    }
    times.sort_by(f64::total_cmp);
    let pct=|p:f64|times[((times.len()-1) as f64*p).round() as usize];
    let elapsed=start.elapsed().as_secs_f64();
    println!("ROI X11 capture: monitor={} {}x{}; faixas={}x{} + {}x{}; {} frames in {:.2}s ({:.1} FPS), {:.1} MB/s; request p50={:.2}ms p95={:.2}ms p99={:.2}ms",monitor_index,m.w,m.h,band_w,m.h,m.w,band_h, times.len(),elapsed,times.len() as f64/elapsed,bytes_per_frame as f64*times.len() as f64/elapsed/1e6,pct(0.50),pct(0.95),pct(0.99));
    Ok(())
}

fn bench_estimators(monitor_index:usize)->Result<(),Box<dyn std::error::Error>>{
    let(conn,screen_index)=x11rb::connect(None)?;let root=conn.setup().roots[screen_index].root;
    let m=*monitors(&conn,root)?.get(monitor_index).ok_or("monitor index is out of range")?;let masks=root_visual_masks(&conn,screen_index)?;
    let(bw,bh)=((m.w/31).max(1),(m.h/17).max(1));let rois=capture_rois(&conn,root,m,bw,bh)?;
    let iterations=250u32;println!("ROI sampler benchmark: DP monitor {} {}x{}, {} frames × 65 LEDs; live X11 image",monitor_index,m.w,m.h,iterations);
    let reference:[Rgb;65]=std::array::from_fn(|led|sample_screen_rois(&rois,m,led,"box",masks));
    for method in ["nearest","grid","box","gaussian","median","luma"]{
        let mut error=0u64;for led in 0..65{let c=sample_screen_rois(&rois,m,led,method,masks);let r=reference[led];error+=c.r.abs_diff(r.r) as u64+c.g.abs_diff(r.g) as u64+c.b.abs_diff(r.b) as u64;}
        let start=Instant::now();let mut checksum=0u64;
        for _ in 0..iterations{for led in 0..65{let c=sample_screen_rois(&rois,m,led,method,masks);checksum=checksum.wrapping_add(c.r as u64+3*c.g as u64+7*c.b as u64);}}
        let total=start.elapsed();println!("{method:10} {:8.2} us/frame ({:6.2} us/LED) MAE-vs-box={:.2}/255 checksum={checksum}",total.as_secs_f64()*1e6/iterations as f64,total.as_secs_f64()*1e6/(iterations as f64*65.0),error as f64/(65.0*3.0));
    }
    println!("resize     measured with the live FFmpeg 320×180 area path; compare using sync ... resize");Ok(())
}

#[derive(Clone,Copy)]
struct VisualMasks{red:u32,green:u32,blue:u32,bits_per_pixel:u8,byte_order:x11rb::protocol::xproto::ImageOrder}
struct Roi{width:usize,height:usize,stride:usize,root_x:i32,root_y:i32,data:Vec<u8>}
struct ScreenRois{left:Roi,top:Roi,right:Roi}

fn capture_rois(conn:&RustConnection,root:Window,m:Monitor,bw:u16,bh:u16)->Result<ScreenRois,Box<dyn std::error::Error>>{
    let sx=(m.x as i32+m.w as i32*2/100) as i16;
    let tx=(m.x as i32+m.w as i32-m.w as i32*2/100-bw as i32) as i16;
    let ty=(m.y as i32+m.h as i32*2/100) as i16;
    let l=conn.get_image(ImageFormat::Z_PIXMAP,root,sx,m.y,bw,m.h,u32::MAX)?;
    let t=conn.get_image(ImageFormat::Z_PIXMAP,root,m.x,ty,m.w,bh,u32::MAX)?;
    let r=conn.get_image(ImageFormat::Z_PIXMAP,root,tx,m.y,bw,m.h,u32::MAX)?;
    let lr=l.reply()?;let tr=t.reply()?;let rr=r.reply()?;
    let make=|w:u16,h:u16,x:i32,y:i32,data:Vec<u8>|Roi{width:w as usize,height:h as usize,stride:w as usize*4,root_x:x,root_y:y,data};
    Ok(ScreenRois{left:make(bw,m.h,sx as i32,m.y as i32,lr.data),top:make(m.w,bh,m.x as i32,ty as i32,tr.data),right:make(bw,m.h,tx as i32,m.y as i32,rr.data)})
}

#[inline]
fn roi_pixel(roi:&Roi,x:usize,y:usize,masks:VisualMasks)->Rgb{
    let i=y.min(roi.height-1)*roi.stride+x.min(roi.width-1)*4;
    let bytes=[roi.data[i],roi.data[i+1],roi.data[i+2],roi.data[i+3]];
    let pixel=if masks.byte_order==x11rb::protocol::xproto::ImageOrder::LSB_FIRST{u32::from_le_bytes(bytes)}else{u32::from_be_bytes(bytes)};
    let channel=|mask:u32|{if mask==0{0}else{let shift=mask.trailing_zeros();let max=mask>>shift;(((pixel&mask)>>shift)*255/max) as u8}};
    let _=masks.bits_per_pixel;
    Rgb{r:channel(masks.red),g:channel(masks.green),b:channel(masks.blue)}
}

fn blend_cursor(rois:&mut ScreenRois,cursor:&x11rb::protocol::xfixes::GetCursorImageReply,masks:VisualMasks){
    let left=cursor.x as i32-cursor.xhot as i32;let top=cursor.y as i32-cursor.yhot as i32;
    for y in 0..cursor.height as usize{for x in 0..cursor.width as usize{
        let src=cursor.cursor_image[y*cursor.width as usize+x];let alpha=(src>>24) as u8;if alpha==0{continue}
        let color=Rgb{r:(src>>16) as u8,g:(src>>8) as u8,b:src as u8};let sx=left+x as i32;let sy=top+y as i32;
        for roi in [&mut rois.left,&mut rois.top,&mut rois.right]{let rx=sx-roi.root_x;let ry=sy-roi.root_y;if rx<0||ry<0||rx>=roi.width as i32||ry>=roi.height as i32{continue}
            let i=ry as usize*roi.stride+rx as usize*4;let bg=roi_pixel(roi,rx as usize,ry as usize,masks);let a=alpha as u32;let mix=|f:u8,b:u8|((f as u32*a+b as u32*(255-a)+127)/255) as u8;let out=Rgb{r:mix(color.r,bg.r),g:mix(color.g,bg.g),b:mix(color.b,bg.b)};
            let pack=|v:u8,mask:u32|{if mask==0{0}else{let shift=mask.trailing_zeros();(((v as u32*(mask>>shift)+127)/255)<<shift)&mask}};let pixel=pack(out.r,masks.red)|pack(out.g,masks.green)|pack(out.b,masks.blue);let bytes=if masks.byte_order==x11rb::protocol::xproto::ImageOrder::LSB_FIRST{pixel.to_le_bytes()}else{pixel.to_be_bytes()};roi.data[i..i+4].copy_from_slice(&bytes);
        }
    }}
}

fn sample_roi(roi:&Roi,rect:(usize,usize,usize,usize),method:&str,masks:VisualMasks)->Rgb{
    let (x0,x1,y0,y1)=rect;let x=(x0+x1-1)/2;let y=(y0+y1-1)/2;
    if method=="nearest"{return roi_pixel(roi,x,y,masks)}
    if method=="grid"{
        let nx=(x1-x0).min(8);let ny=(y1-y0).min(8);let(mut r,mut g,mut b)=(0u64,0u64,0u64);
        for gy in 0..ny{let sy=y0+((2*gy+1)*(y1-y0)/(2*ny)).min(y1-y0-1);for gx in 0..nx{let sx=x0+((2*gx+1)*(x1-x0)/(2*nx)).min(x1-x0-1);let p=roi_pixel(roi,sx,sy,masks);r+=p.r as u64;g+=p.g as u64;b+=p.b as u64;}}
        let n=(nx*ny).max(1) as u64;return Rgb{r:(r/n) as u8,g:(g/n) as u8,b:(b/n) as u8};
    }
    if method=="gaussian"||method=="median"{
        let mut rs=[0u8;9];let mut gs=[0u8;9];let mut bs=[0u8;9];let mut n=0;
        for dy in 0..3{for dx in 0..3{let p=roi_pixel(roi,(x+dx).saturating_sub(1), (y+dy).saturating_sub(1),masks);rs[n]=p.r;gs[n]=p.g;bs[n]=p.b;n+=1;}}
        if method=="median"{rs.sort_unstable();gs.sort_unstable();bs.sort_unstable();return Rgb{r:rs[4],g:gs[4],b:bs[4]}}
        const K:[u32;9]=[1,2,1,2,4,2,1,2,1];let(mut r,mut g,mut b)=(0u32,0u32,0u32);
        for i in 0..9{r+=rs[i] as u32*K[i];g+=gs[i] as u32*K[i];b+=bs[i] as u32*K[i];}
        return Rgb{r:(r/16) as u8,g:(g/16) as u8,b:(b/16) as u8};
    }
    let(mut r,mut g,mut b,mut count)=(0u64,0u64,0u64,0u64);let(mut wr,mut wg,mut wb,mut total)=(0u64,0u64,0u64,0u64);
    for sy in y0..y1{for sx in x0..x1{let p=roi_pixel(roi,sx,sy,masks);if method=="luma"{let lum=(54*p.r as u64+183*p.g as u64+19*p.b as u64)>>8;let weight=32+lum;wr+=p.r as u64*weight;wg+=p.g as u64*weight;wb+=p.b as u64*weight;total+=weight;}else{r+=p.r as u64;g+=p.g as u64;b+=p.b as u64;count+=1;}}}
    if method=="luma"{Rgb{r:(wr/total.max(1)) as u8,g:(wg/total.max(1)) as u8,b:(wb/total.max(1)) as u8}}else{Rgb{r:(r/count.max(1)) as u8,g:(g/count.max(1)) as u8,b:(b/count.max(1)) as u8}}
}

#[inline]
fn blend_colors(current:Rgb,target:Rgb,alpha:f32)->Rgb{
    let f=|a:u8,b:u8|((a as f32+(b as f32-a as f32)*alpha).round().clamp(0.0,255.0)) as u8;
    Rgb{r:f(current.r,target.r),g:f(current.g,target.g),b:f(current.b,target.b)}
}

fn sample_screen_rois(rois:&ScreenRois,m:Monitor,led:usize,method:&str,masks:VisualMasks)->Rgb{
    let bw=rois.left.width;let bh=rois.top.height;let w=m.w as usize;let h=m.h as usize;
    if led<17{let row=16-led;sample_roi(&rois.left,(0,bw,row*h/17,(row+1)*h/17),method,masks)}
    else if led<48{let col=led-17;sample_roi(&rois.top,(col*w/31,(col+1)*w/31,0,bh),method,masks)}
    else{let row=led-48;sample_roi(&rois.right,(0,bw,row*h/17,(row+1)*h/17),method,masks)}
}

fn root_visual_masks(conn:&RustConnection,screen_index:usize)->Result<VisualMasks,Box<dyn std::error::Error>>{
    let screen=&conn.setup().roots[screen_index];
    let format=conn.setup().pixmap_formats.iter().find(|f|f.depth==screen.root_depth).ok_or("X11 pixel format missing")?;
    let visual=screen.allowed_depths.iter().flat_map(|d|d.visuals.iter()).find(|v|v.visual_id==screen.root_visual).ok_or("X11 root visual missing")?;
    if format.bits_per_pixel!=32{return Err(format!("unsupported X11 pixel width: {} bpp",format.bits_per_pixel).into())}
    Ok(VisualMasks{red:visual.red_mask,green:visual.green_mask,blue:visual.blue_mask,bits_per_pixel:format.bits_per_pixel,byte_order:conn.setup().image_byte_order})
}

fn roi_screen_sync(path:&str,monitor_index:usize,fps:u32,estimator:&str,inverted:bool,contrast:f32,strength:f32,transition_ms:f32,preview_enabled:bool)->Result<(),Box<dyn std::error::Error>>{
    let device_path=if path=="auto"{find_control_hidraw()?}else{PathBuf::from(path)};
    let mut device=OpenOptions::new().write(true).open(&device_path)?;let mut packet_id=2u8;
    let (conn,screen_index)=x11rb::connect(None)?;
    let mut cursor_supported=match conn.xfixes_query_version(5,0){Ok(cookie)=>match cookie.reply(){Ok(_)=>true,Err(e)=>{eprintln!("cursor overlay unavailable ({e}); continuing without XFixes");false}},Err(e)=>{eprintln!("cursor overlay unavailable ({e}); continuing without XFixes");false}};
    let root=conn.setup().roots[screen_index].root;let masks=root_visual_masks(&conn,screen_index)?;
    let m=*monitors(&conn,root)?.get(monitor_index).ok_or("monitor index is out of range")?;
    let band_w=(m.w/31).max(1);let band_h=(m.h/17).max(1);let rate=fps.clamp(1,120);let period=Duration::from_secs_f64(1.0/rate as f64);
    eprintln!("screen sync: monitor={} geometry={}x{}+{}+{}, estimator={estimator}, capture=X11-ROI ({band_w}x{} + {}x{band_h} + {band_w}x{}), target={rate}Hz",monitor_index,m.w,m.h,m.x,m.y,m.h,m.w,m.h);

    let preview_slot=Arc::new(Mutex::new((vec![0u8;320*180*4],0u64)));
    let mut preview_child=None;
    if preview_enabled{
        let display=env::var("DISPLAY").unwrap_or_else(|_|":0".into());let mut cmd=Command::new("ffmpeg");
        cmd.args(["-hide_banner","-loglevel","fatal","-thread_queue_size","1","-f","x11grab","-draw_mouse","1","-framerate","10","-video_size"])
            .arg(format!("{}x{}",m.w,m.h)).args(["-i"]).arg(format!("{display}+{},{}",m.x,m.y))
            .args(["-vf","fps=10,scale=320:180:flags=area","-an","-f","rawvideo","-pix_fmt","bgr0","pipe:1"])
            .stdout(Stdio::piped()).stderr(Stdio::null());
        unsafe{cmd.pre_exec(||{libc::prctl(libc::PR_SET_PDEATHSIG,libc::SIGTERM);Ok(())});}
        let mut child=cmd.spawn()?;let mut output=child.stdout.take().ok_or("preview capture pipe unavailable")?;let slot=Arc::clone(&preview_slot);
        thread::Builder::new().name("screen-preview".into()).spawn(move||{let mut next=vec![0u8;320*180*4];loop{if let Err(e)=output.read_exact(&mut next){eprintln!("preview capture stopped: {e}");break}if let Ok(mut latest)=slot.lock(){std::mem::swap(&mut latest.0,&mut next);latest.1+=1;}}})?;
        preview_child=Some(child);
    }

    let mut preview_frame=vec![0u8;320*180*4];let mut preview_seq=0u64;let mut last:Option<[Rgb;65]>=None;let mut smoothed=[Rgb::default();65];let mut previous_tick=Instant::now();let mut first_sample=true;
    let mut frames=0u64;let mut sent=0u64;let mut capture_ns=0u128;let mut sample_ns=0u128;let mut hid_ns=0u128;
    let mut latencies=Vec::<f64>::with_capacity(128);let mut stats=Instant::now();let mut stats_seq=0u64;
    loop{
        let tick=Instant::now();let c=Instant::now();let mut rois=capture_rois(&conn,root,m,band_w,band_h)?;
        if cursor_supported{match conn.xfixes_get_cursor_image(){Ok(cookie)=>match cookie.reply(){Ok(cursor)=>blend_cursor(&mut rois,&cursor,masks),Err(e)=>{eprintln!("cursor overlay disabled ({e})");cursor_supported=false}},Err(e)=>{eprintln!("cursor overlay disabled ({e})");cursor_supported=false}}}
        capture_ns+=c.elapsed().as_nanos();
        let s=Instant::now();let mut target=[Rgb::default();65];
        for led in 0..65{let physical=if inverted{64-led}else{led};target[led]=apply_picture_controls(sample_screen_rois(&rois,m,physical,estimator,masks),contrast,strength);}
        let elapsed=previous_tick.elapsed().as_secs_f32()*1000.0;previous_tick=Instant::now();let alpha=if transition_ms<=0.0||first_sample{1.0}else{1.0-(-elapsed/transition_ms).exp()};first_sample=false;let mut colors=[Rgb::default();65];
        for i in 0..65{smoothed[i]=blend_colors(smoothed[i],target[i],alpha);colors[i]=smoothed[i];}
        sample_ns+=s.elapsed().as_nanos();frames+=1;
        if preview_enabled&&frames%6==0{
            let changed={if let Ok(mut slot)=preview_slot.lock(){if slot.1!=preview_seq{std::mem::swap(&mut preview_frame,&mut slot.0);preview_seq=slot.1;true}else{false}}else{false}};
            if changed{let mut packet=Vec::with_capacity(320*180*3+65*3);for p in preview_frame.chunks_exact(4){packet.extend_from_slice(&[p[2],p[1],p[0]]);}for c in &colors{packet.extend_from_slice(&[c.r,c.g,c.b]);}let mut out=io::stdout().lock();out.write_all(&packet)?;out.flush()?;}
        }
        if last.as_ref()!=Some(&colors){let h=Instant::now();send_led_colors(&mut device,&mut packet_id,&colors)?;hid_ns+=h.elapsed().as_nanos();latencies.push(tick.elapsed().as_secs_f64()*1000.0);sent+=1;last=Some(colors);}
        if stats.elapsed()>=Duration::from_secs(2){let seconds=stats.elapsed().as_secs_f64();let sequence=frames+stats_seq;latencies.sort_by(f64::total_cmp);let p=|q:f64|if latencies.is_empty(){0.0}else{latencies[((latencies.len()-1) as f64*q).round() as usize]};
            if sent==0{eprintln!("stats: frames={} tx=0 dropped=0 capture_fps={:.1} capture={:.3}ms sample={:.3}ms hid_write=n/a frame_read_to_hid_write_p95=n/a",frames,frames as f64/seconds,capture_ns as f64/1e6/frames.max(1) as f64,sample_ns as f64/1e6/frames.max(1) as f64)}
            else{eprintln!("stats: frames={} tx={} dropped=0 capture_fps={:.1} capture={:.3}ms sample={:.3}ms hid_write={:.2}ms frame_read_to_hid_write_p50={:.2}ms frame_read_to_hid_write_p95={:.2}ms frame_read_to_hid_write_p99={:.2}ms",frames,sent,frames as f64/seconds,capture_ns as f64/1e6/frames.max(1) as f64,sample_ns as f64/1e6/frames.max(1) as f64,hid_ns as f64/1e6/sent.max(1) as f64,p(0.50),p(0.95),p(0.99));}
            stats_seq=sequence;frames=0;sent=0;capture_ns=0;sample_ns=0;hid_ns=0;latencies.clear();stats=Instant::now();}
        if let Some(remaining)=period.checked_sub(tick.elapsed()){thread::sleep(remaining);}
        let _=&preview_child;
    }
}

fn ffmpeg_screen_sync(path: &str, monitor_index: usize, fps: u32, estimator: &str, inverted: bool, contrast: f32, strength: f32, transition_ms:f32, preview_enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    let device_path=if path=="auto"{find_control_hidraw()?}else{PathBuf::from(path)};
    let (conn, screen_num) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen_num].root;
    let mons = monitors(&conn, root)?;
    let m = *mons.get(monitor_index).ok_or("monitor index is out of range")?;
    let mut device = OpenOptions::new().write(true).open(&device_path)?;
    let mut packet_id = 2u8;
    eprintln!("screen sync: monitor={} geometry={}x{}+{}+{}, estimator={estimator}, target={fps}Hz", monitor_index, m.w, m.h, m.x, m.y);
    let display = env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
    let resized = estimator == "resize";
    let (capture_w,capture_h)=if resized {(320usize,180usize)}else{(m.w as usize,m.h as usize)};
    let algorithm=if resized {"box"}else{estimator};
    let mut ffmpeg_cmd = Command::new("ffmpeg");
    ffmpeg_cmd
        .args(["-hide_banner", "-loglevel", "error", "-thread_queue_size", "1", "-f", "x11grab", "-draw_mouse", "1", "-framerate"])
        .arg(fps.max(1).min(120).to_string())
        .args(["-video_size"]).arg(format!("{}x{}", m.w, m.h))
        .args(["-i"]).arg(format!("{display}+{},{}", m.x, m.y))
        .args(["-an"]);
    if resized { ffmpeg_cmd.arg("-vf").arg(format!("fps={},scale=320:180:flags=area",fps.max(1).min(120))); }
    ffmpeg_cmd.args(["-f", "rawvideo", "-pix_fmt", "bgr0", "pipe:1"])
        .stdout(Stdio::piped()).stderr(Stdio::inherit());
    unsafe { ffmpeg_cmd.pre_exec(|| { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM); Ok(()) }); }
    let mut ffmpeg = ffmpeg_cmd.spawn()?;
    let mut capture = ffmpeg.stdout.take().ok_or("ffmpeg stdout unavailable")?;
    let frame_bytes = capture_w * capture_h * 4;
    // A single latest-frame slot bounds latency. Capture keeps running while
    // the USB device consumes the previous update; stale frames are replaced.
    let shared = Arc::new((Mutex::new((vec![0u8; frame_bytes], 0u64, Instant::now())), Condvar::new()));
    let capture_slot = Arc::clone(&shared);
    let _capture_thread = thread::Builder::new().name("screen-capture".into()).spawn(move || {
        let mut next = vec![0u8; frame_bytes];
        loop {
            // Never hold the consumer mutex across a blocking pipe read.
            if let Err(error) = capture.read_exact(&mut next) {
                eprintln!("capture stopped: {error}");
                break;
            }
            let (lock, ready) = &*capture_slot;
            let Ok(mut latest) = lock.lock() else { break };
            std::mem::swap(&mut latest.0, &mut next);
            latest.1 += 1;
            latest.2 = Instant::now();
            ready.notify_one();
        }
    })?;
    let mut frame = vec![0u8; frame_bytes];
    let mut last:Option<[Rgb;65]>=None;let mut smoothed=[Rgb::default();65];let mut previous_sample=Instant::now();let mut first_sample=true;
    let mut frames = 0u64; let mut sent = 0u64; let mut skipped = 0u64;
    let mut compute_ns = 0u128;
    let mut hid_ns = 0u128;
    let mut latencies_ms:Vec<f64>=Vec::with_capacity(128);
    let mut last_stats = Instant::now();
    let mut last_sequence = 0u64;
    let mut stats_sequence = 0u64;
    loop {
        let (lock, ready) = &*shared;
        let mut latest = lock.lock().map_err(|_| "capture frame lock poisoned")?;
        while latest.1 == last_sequence {
            latest = ready.wait(latest).map_err(|_| "capture frame lock poisoned")?;
        }
        if last_sequence != 0 { skipped += latest.1.saturating_sub(last_sequence + 1); }
        last_sequence = latest.1;
        std::mem::swap(&mut frame, &mut latest.0);
        let captured_at = latest.2;
        drop(latest);
        let t = Instant::now();
        let image = Frame { width: capture_w, height: capture_h, rgba: &frame };
        let mut target = [Rgb::default(); 65];
        for led in 0..65usize {
            let physical_led = if inverted { 64 - led } else { led };
            let p = apply_picture_controls(sample_edge_bgr0(image, physical_led, algorithm), contrast, strength);
            target[led] = p;
        }
        let now=Instant::now();let elapsed=now.duration_since(previous_sample).as_secs_f32()*1000.0;previous_sample=now;
        let alpha=if transition_ms<=0.0||first_sample{1.0}else{1.0-(-elapsed/transition_ms).exp()};first_sample=false;
        let mut colors=[Rgb::default();65];for i in 0..65{smoothed[i]=blend_colors(smoothed[i],target[i],alpha);colors[i]=smoothed[i];}
        compute_ns += t.elapsed().as_nanos();
        frames += 1;
        if preview_enabled && frames % 6 == 0 {
            // Preview is independent of LED sampling and carries the pointer.
            // 320x180 at 10 Hz keeps it crisp without resizing the LED source.
            // Fixed-size preview (320x180 RGB + 65 sampled LED colors),
            // delivered at 10 Hz; this is small and does not trigger another capture.
            let mut packet=Vec::with_capacity(320*180*3+65*3);
            for py in 0..180usize { let sy=py*image.height/180; for px in 0..320usize { let sx=px*image.width/320;let c=image.pixel(sx,sy);packet.extend_from_slice(&[c.r,c.g,c.b]); }}
            for c in &colors { packet.extend_from_slice(&[c.r,c.g,c.b]); }
            let mut stdout=io::stdout().lock();stdout.write_all(&packet)?;stdout.flush()?;
        }
        if last.as_ref()!=Some(&colors) {
            let t = Instant::now();
            send_led_colors(&mut device, &mut packet_id, &colors)?;
            hid_ns += t.elapsed().as_nanos();
            latencies_ms.push(captured_at.elapsed().as_secs_f64()*1000.0);
            sent += 1;
            last = Some(colors);
        }
        if last_stats.elapsed() >= Duration::from_secs(2) {
            let secs = last_stats.elapsed().as_secs_f64();
            latencies_ms.sort_by(f64::total_cmp);
            let percentile=|p:f64|if latencies_ms.is_empty(){0.0}else{latencies_ms[((latencies_ms.len()-1) as f64*p).round() as usize]};
            let (slot, _) = &*shared;
            let captured_total = slot.lock().map(|s| s.1).unwrap_or(last_sequence);
            if sent == 0 {
                eprintln!("stats: frames={} tx=0 dropped={} capture_fps={:.1} sample={:.3}ms hid_write=n/a frame_read_to_hid_write_p95=n/a", frames, skipped, captured_total.saturating_sub(stats_sequence) as f64 / secs, compute_ns as f64 / 1e6 / frames.max(1) as f64);
            } else {
                eprintln!("stats: frames={} tx={} dropped={} capture_fps={:.1} sample={:.3}ms hid_write={:.2}ms frame_read_to_hid_write_p50={:.2}ms frame_read_to_hid_write_p95={:.2}ms frame_read_to_hid_write_p99={:.2}ms", frames, sent, skipped, captured_total.saturating_sub(stats_sequence) as f64 / secs, compute_ns as f64 / 1e6 / frames.max(1) as f64, hid_ns as f64 / 1e6 / sent.max(1) as f64,percentile(0.50),percentile(0.95),percentile(0.99));
            }
            stats_sequence = captured_total;
            frames = 0; sent = 0; skipped = 0; compute_ns = 0; hid_ns = 0;latencies_ms.clear(); last_stats = Instant::now();
        }
    }
    #[allow(unreachable_code)]
    { let _ = ffmpeg.kill(); Ok(()) }
}

fn screen_sync(path:&str,monitor:usize,fps:u32,estimator:&str,inverted:bool,contrast:f32,strength:f32,transition_ms:f32,preview:bool)->Result<(),Box<dyn std::error::Error>>{
    if estimator=="resize"{ffmpeg_screen_sync(path,monitor,fps,estimator,inverted,contrast,strength,transition_ms,preview)}
    else{roi_screen_sync(path,monitor,fps,estimator,inverted,contrast,strength,transition_ms,preview)}
}

fn find_control_hidraw()->Result<PathBuf,Box<dyn std::error::Error>>{
    let expected=format!("0003:{VID:08X}:{PID:08X}");
    for entry in fs::read_dir("/sys/bus/hid/devices")?{
        let dir=entry?.path();let Ok(info)=fs::read_to_string(dir.join("uevent"))else{continue};
        if !info.to_uppercase().contains(&expected){continue;}
        let interface=fs::canonicalize(&dir)?.parent().map(PathBuf::from).ok_or("missing USB interface")?;
        if fs::read_to_string(interface.join("bInterfaceNumber")).ok().as_deref()!=Some("00\n"){continue;}
        // Ignore the touch-button keyboard interface; control is usage page FF00.
        let hidraw=dir.join("hidraw");if let Ok(nodes)=fs::read_dir(hidraw){for node in nodes.flatten(){let name=node.file_name();let path=PathBuf::from("/dev").join(name);if path.exists(){return Ok(path);}}}
    }
    Err("compatible ROBOBLOQ USB HID interface not found; verify udev access".into())
}

#[inline]
fn apply_picture_controls(c: Rgb, contrast: f32, strength: f32) -> Rgb {
    let f = |v: u8| (((v as f32 - 127.5) * contrast + 127.5) * strength).round().clamp(0.0, 255.0) as u8;
    Rgb { r:f(c.r), g:f(c.g), b:f(c.b) }
}

fn sample_edge_bgr0(frame: Frame<'_>, led: usize, estimator: &str) -> Rgb {
    let w=frame.width; let h=frame.height;
    // Match each LED to its own rectangle around the 3-sided screen edge:
    // 17 bottom-to-top on the left, 31 left-to-right on top, 17 top-to-bottom on right.
    let band_w=(w/31).max(1);let band_h=(h/17).max(1);
    let (x0,x1,y0,y1)=if led<17 {
        let row=16-led;(w*2/100,(w*2/100+band_w).min(w),row*h/17,(row+1)*h/17)
    }else if led<48 {
        let col=led-17;(col*w/31,(col+1)*w/31,h*2/100,(h*2/100+band_h).min(h))
    }else{
        let row=led-48;(w-w*2/100-band_w,w-w*2/100,row*h/17,(row+1)*h/17)
    };
    let x=(x0+x1.saturating_sub(1))/2;let y=(y0+y1.saturating_sub(1))/2;
    if estimator=="nearest" { return frame.pixel(x,y); }
    if estimator=="box" || estimator=="luma" {
        let(mut r,mut g,mut b)=(0u64,0u64,0u64);let mut count=0u64;let(mut wr,mut wg,mut wb,mut total)=(0u64,0u64,0u64,0u64);
        for sy in y0..y1 {for sx in x0..x1 {let p=frame.pixel(sx,sy);if estimator=="box"{r+=p.r as u64;g+=p.g as u64;b+=p.b as u64;count+=1}else{let lum=(54*p.r as u64+183*p.g as u64+19*p.b as u64)>>8;let weight=32+lum;wr+=p.r as u64*weight;wg+=p.g as u64*weight;wb+=p.b as u64*weight;total+=weight;}}}
        if estimator=="box" {return if count==0{Rgb::default()}else{Rgb{r:(r/count) as u8,g:(g/count) as u8,b:(b/count) as u8}}}
        return if total==0{Rgb::default()}else{Rgb{r:(wr/total) as u8,g:(wg/total) as u8,b:(wb/total) as u8}};
    }
    let radius=if estimator=="gaussian" || estimator=="median" {1} else {3};
    if estimator == "gaussian" {
        const K: [[u32;3];3] = [[1,2,1],[2,4,2],[1,2,1]];
        let (mut r,mut g,mut b)=(0u32,0u32,0u32);
        for (dy,row) in K.iter().enumerate() { for (dx,&weight) in row.iter().enumerate() {
            let p=frame.pixel((x+dx).saturating_sub(1).min(w-1),(y+dy).saturating_sub(1).min(h-1));
            r+=p.r as u32*weight;g+=p.g as u32*weight;b+=p.b as u32*weight;
        }}
        return Rgb{r:(r/16) as u8,g:(g/16) as u8,b:(b/16) as u8};
    }
    let mut r=0u32; let mut g=0u32; let mut b=0u32; let mut n=0u32;
    let mut rs=[0u8;49];let mut gs=[0u8;49];let mut bs=[0u8;49];
    for dy in 0..radius*2+1 { for dx in 0..radius*2+1 {
        let p=frame.pixel((x+dx).saturating_sub(radius).min(w-1),(y+dy).saturating_sub(radius).min(h-1));
        rs[n as usize]=p.r;gs[n as usize]=p.g;bs[n as usize]=p.b;
        r+=p.r as u32;g+=p.g as u32;b+=p.b as u32;n+=1;
    }}
    if estimator=="median" {
        rs[..9].sort_unstable();gs[..9].sort_unstable();bs[..9].sort_unstable();
        return Rgb{r:rs[4],g:gs[4],b:bs[4]};
    }
    if estimator=="luma" {
        let(mut wr,mut wg,mut wb,mut total)=(0u64,0u64,0u64,0u64);
        for i in 0..n as usize {let lum=(54*rs[i] as u64+183*gs[i] as u64+19*bs[i] as u64)>>8;let weight=32+lum;wr+=rs[i] as u64*weight;wg+=gs[i] as u64*weight;wb+=bs[i] as u64*weight;total+=weight;}
        return Rgb{r:(wr/total) as u8,g:(wg/total) as u8,b:(wb/total) as u8};
    }
    Rgb{r:(r/n) as u8,g:(g/n) as u8,b:(b/n) as u8}
}

fn bench() {
    let width = 1920;
    let height = 1080;
    let mut pixels = vec![0u8; width * height * 4];
    for (i, p) in pixels.chunks_exact_mut(4).enumerate() {
        let x = (i % width) as u32;
        let y = (i / width) as u32;
        p[0] = ((x * 255) / width as u32) as u8;
        p[1] = ((y * 255) / height as u32) as u8;
        p[2] = ((x ^ y) & 255) as u8;
        p[3] = 255;
    }
    let frame = Frame { width, height, rgba: &pixels };
    let iterations = 500;
    let cases: [(&str, Box<dyn Fn(usize) -> Rgb>); 3] = [
        ("box-edge", Box::new(|i| box_edge(frame, i & 63, i & 63, 64, 64))),
        ("gaussian-3x3", Box::new(|i| gaussian3(frame, (i * 17) % (width - 2) + 1, (i * 31) % (height - 2) + 1))),
        ("luma-weighted", Box::new(|i| luma_weighted(frame, i & 63, i & 63, 64, 64))),
    ];
    println!("benchmark: {width}x{height}, {iterations} iterations, synthetic frame");
    for (name, f) in cases {
        let start = Instant::now();
        let mut value = Rgb::default();
        for i in 0..iterations { value = std::hint::black_box(f(i)); }
        let elapsed = start.elapsed();
        let per = elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64;
        println!("{name:16} {per:8.2} us/sample  result={value:?}");
    }
}

fn usage() {
    eprintln!("usage: synclights [bench | bench-roi MONITOR | bench-algorithms MONITOR | probe /dev/hidraw7 | color /dev/hidraw7 R G B | sync /dev/hidraw7 MONITOR FPS [grid|nearest|box|gaussian|median|luma|resize] [normal|invert] [contrast:0.5..2] [strength:0..2] [preview] [transition_ms]]");
}

fn main() {
    match env::args().nth(1).as_deref() {
        Some("bench") => bench(),
        Some("bench-roi") => {
            let index=env::args().nth(2).and_then(|v|v.parse().ok()).unwrap_or(0);
            if let Err(e)=bench_roi_capture(index){eprintln!("ROI capture benchmark failed: {e}");std::process::exit(2);}
        }
        Some("bench-algorithms")=>{let index=env::args().nth(2).and_then(|v|v.parse().ok()).unwrap_or(0);if let Err(e)=bench_estimators(index){eprintln!("estimator benchmark failed: {e}");std::process::exit(2);}}
        Some("probe") => match env::args().nth(2) {
            Some(path) => probe(&path).unwrap_or_else(|e| { eprintln!("probe failed: {e}"); std::process::exit(2); }),
            None => usage(),
        },
        Some("color") => {
            let args: Vec<_> = env::args().skip(2).collect();
            if args.len() != 4 { usage(); return; }
            let parse = |s: &str| u8::from_str_radix(s.trim_start_matches("0x"), 16)
                .or_else(|_| s.parse::<u8>()).expect("RGB values must be 0..255");
            set_color(&args[0], Rgb { r: parse(&args[1]), g: parse(&args[2]), b: parse(&args[3]) })
                .unwrap_or_else(|e| { eprintln!("color failed: {e}"); std::process::exit(2); });
        }
        Some("sync") => {
            let args: Vec<_> = env::args().skip(2).collect();
            if args.len() < 3 { usage(); return; }
            let m = args[1].parse().expect("monitor index must be an integer");
            let fps = args[2].parse().expect("FPS must be an integer");
            let alg = args.get(3).map(String::as_str).unwrap_or("box");
            let inv = args.get(4).is_some_and(|v| v == "invert");
            let contrast = args.get(5).and_then(|v|v.parse().ok()).unwrap_or(1.0);
            let strength = args.get(6).and_then(|v|v.parse().ok()).unwrap_or(1.0);
            let preview=args.get(7).is_some_and(|v|v=="preview");
            let transition=args.get(8).and_then(|v|v.parse().ok()).unwrap_or(0.0);
            if let Err(e) = screen_sync(&args[0], m, fps, alg, inv, contrast, strength, transition, preview) { eprintln!("sync failed: {e}"); std::process::exit(2); }
        }
        _ => usage(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rb_checksum_and_size_are_correct() {
        let mut id = 2;
        let frame = rb_frame(&mut id, 134, &[1, 255, 0, 0, 254]);
        assert_eq!(frame.len(), 11);
        assert_eq!(frame[0..2], *b"RB");
        assert_eq!(frame[2], 11);
        assert_eq!(frame[3], 2);
        assert_eq!(frame[4], 134);
        assert_eq!(frame[10], checksum(&frame[..10]));
        assert_eq!(id, 3);
    }

    #[test]
    fn hid_report_has_zero_report_id_and_fixed_size() {
        let report = hid_report(&[b'R', b'B', 6, 2, 130, 0]);
        assert_eq!(report.len(), HID_REPORT);
        assert_eq!(report[0], 0);
        assert_eq!(&report[1..7], b"RB\x06\x02\x82\0");
        assert!(report[7..].iter().all(|v| *v == 0));
    }

    #[test]
    fn screen_frames_use_the_official_sc_header_and_increment_packet_id() {
        let colors=[Rgb{r:1,g:2,b:3};65];
        let mut id=2;
        let first=screen_frame(&mut id,&colors);
        let second=screen_frame(&mut id,&colors);
        assert_eq!(&first[..2],b"SC");
        assert_eq!(&first[2..4],&(332u16).to_be_bytes());
        assert_eq!(&first[4..6],&[2,128]);
        assert_eq!(&first[6..11],&[1,1,2,3,1]);
        assert_eq!(first[331],checksum(&first[..331]));
        assert_eq!(second[4],3);
        assert_eq!(id,4);
    }

    #[test]
    fn screen_packet_id_wraps_like_the_official_counter() {
        let mut id=254;
        let frame=screen_frame(&mut id,&[Rgb::default();65]);
        assert_eq!(frame[4],254);
        assert_eq!(id,1);
    }

    #[test]
    fn roi_sampling_matches_full_frame_for_all_edge_estimators() {
        let (w,h)=(310usize,170usize);let mut pixels=vec![0u8;w*h*4];
        for y in 0..h{for x in 0..w{let i=(y*w+x)*4;pixels[i]=((x*7+y*3)%256) as u8;pixels[i+1]=((y*11)%256) as u8;pixels[i+2]=((x*5)%256) as u8;}}
        let frame=Frame{width:w,height:h,rgba:&pixels};let bw=w/31;let bh=h/17;
        let region=|x0:usize,y0:usize,rw:usize,rh:usize|{let mut data=vec![0u8;rw*rh*4];for y in 0..rh{let a=((y0+y)*w+x0)*4;let b=y*rw*4;data[b..b+rw*4].copy_from_slice(&pixels[a..a+rw*4]);}Roi{width:rw,height:rh,stride:rw*4,root_x:x0 as i32,root_y:y0 as i32,data}};
        let rois=ScreenRois{left:region(w*2/100,0,bw,h),top:region(0,h*2/100,w,bh),right:region(w-w*2/100-bw,0,bw,h)};
        let masks=VisualMasks{red:0x00ff0000,green:0x0000ff00,blue:0x000000ff,bits_per_pixel:32,byte_order:x11rb::protocol::xproto::ImageOrder::LSB_FIRST};
        for estimator in ["nearest","box","gaussian","median","luma"]{for led in 0..65{assert_eq!(sample_screen_rois(&rois,Monitor{x:0,y:0,w:w as u16,h:h as u16},led,estimator,masks),sample_edge_bgr0(frame,led,estimator),"estimator={estimator}, led={led}");}}
    }

    #[test]
    fn stratified_grid_tracks_area_average_on_smooth_gradients(){
        let(w,h)=(61usize,63usize);let mut data=vec![0u8;w*h*4];
        for y in 0..h{for x in 0..w{let i=(y*w+x)*4;data[i]=(x*255/w) as u8;data[i+1]=(y*255/h) as u8;data[i+2]=((x+y)*255/(w+h)) as u8;}}
        let roi=Roi{width:w,height:h,stride:w*4,root_x:0,root_y:0,data};let masks=VisualMasks{red:0x00ff0000,green:0x0000ff00,blue:0x000000ff,bits_per_pixel:32,byte_order:x11rb::protocol::xproto::ImageOrder::LSB_FIRST};
        let exact=sample_roi(&roi,(0,w,0,h),"box",masks);let fast=sample_roi(&roi,(0,w,0,h),"grid",masks);
        for (a,b) in [(exact.r,fast.r),(exact.g,fast.g),(exact.b,fast.b)]{assert!(a.abs_diff(b)<=3,"full area={exact:?}, 8x8 grid={fast:?}");}
    }
}
