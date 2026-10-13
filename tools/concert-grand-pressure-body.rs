//! Loaded body and two causal microphones; no strings/contact yet.
use std::{alloc::{GlobalAlloc, Layout, System}, env, fs, hint::black_box,
    sync::atomic::{AtomicUsize, Ordering}, time::Instant};
struct CountAlloc;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 { ALLOCATIONS.fetch_add(1, Ordering::Relaxed); unsafe { System.alloc(l) } }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { System.dealloc(p,l) } }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 { ALLOCATIONS.fetch_add(1, Ordering::Relaxed); unsafe { System.realloc(p,l,n) } }
}
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;
fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut sums = [0.; 4];
    for (aa, bb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
        for j in 0..4 { sums[j] = aa[j].mul_add(bb[j], sums[j]); }
    }
    let mut result = (sums[0]+sums[1])+(sums[2]+sums[3]);
    for (a,b) in a.chunks_exact(4).remainder().iter().zip(b.chunks_exact(4).remainder()) { result = a.mul_add(*b,result); }
    result
}
#[inline]
fn dot_pair(a: &[f64], b: &[f64], f: &[f64]) -> (f64,f64) {
    let mut r=[0.;4]; let mut i=[0.;4];
    for ((aa,bb),ff) in a.chunks_exact(4).zip(b.chunks_exact(4)).zip(f.chunks_exact(4)) {
        for j in 0..4 { r[j]=aa[j].mul_add(ff[j],r[j]); i[j]=bb[j].mul_add(ff[j],i[j]); }
    }
    let mut real=(r[0]+r[1])+(r[2]+r[3]); let mut imag=(i[0]+i[1])+(i[2]+i[3]);
    for ((a,b),f) in a.chunks_exact(4).remainder().iter().zip(b.chunks_exact(4).remainder()).zip(f.chunks_exact(4).remainder()) {
        real=a.mul_add(*f,real); imag=b.mul_add(*f,imag);
    }
    (real,imag)
}
#[cfg(target_arch="x86_64")]
#[target_feature(enable="avx2,fma")]
unsafe fn dot_avx(a:&[f64],b:&[f64])->f64 {
    use std::arch::x86_64::*;
    let mut sum=_mm256_setzero_pd(); let end=a.len()/4*4;
    for n in (0..end).step_by(4) {
        sum=_mm256_fmadd_pd(unsafe{_mm256_loadu_pd(a.as_ptr().add(n))},unsafe{_mm256_loadu_pd(b.as_ptr().add(n))},sum);
    }
    let mut lanes=[0.;4]; unsafe{_mm256_storeu_pd(lanes.as_mut_ptr(),sum);}
    let mut result=(lanes[0]+lanes[1])+(lanes[2]+lanes[3]);
    for n in end..a.len() { result=a[n].mul_add(b[n],result); }
    result
}
#[inline]
fn mapped_dot(a:&[f64],b:&[f64],simd:bool)->f64 {
    #[cfg(target_arch="x86_64")]
    if simd { return unsafe{dot_avx(a,b)}; }
    dot(a,b)
}
#[cfg(target_arch="x86_64")]
#[target_feature(enable="avx2,fma")]
unsafe fn dot_pair_avx(a:&[f64],b:&[f64],f:&[f64])->(f64,f64) {
    use std::arch::x86_64::*;
    let mut r=_mm256_setzero_pd(); let mut i=r;
    let end=a.len()/4*4;
    for n in (0..end).step_by(4) {
        let input=unsafe{_mm256_loadu_pd(f.as_ptr().add(n))};
        r=_mm256_fmadd_pd(unsafe{_mm256_loadu_pd(a.as_ptr().add(n))},input,r);
        i=_mm256_fmadd_pd(unsafe{_mm256_loadu_pd(b.as_ptr().add(n))},input,i);
    }
    let mut rr=[0.;4]; let mut ii=[0.;4];
    unsafe{_mm256_storeu_pd(rr.as_mut_ptr(),r); _mm256_storeu_pd(ii.as_mut_ptr(),i);}
    let mut r=(rr[0]+rr[1])+(rr[2]+rr[3]); let mut i=(ii[0]+ii[1])+(ii[2]+ii[3]);
    for n in end..a.len() { r=a[n].mul_add(f[n],r); i=b[n].mul_add(f[n],i); }
    (r,i)
}
#[cfg(target_arch="x86_64")]
#[target_feature(enable="avx2,fma")]
unsafe fn accumulate_avx(out:&mut[f64],a:&[f64],b:&[f64],r:f64,i:f64) {
    use std::arch::x86_64::*;
    let r=_mm256_set1_pd(r); let i=_mm256_set1_pd(i);
    let end=out.len()/4*4;
    for n in (0..end).step_by(4) {
        let old=unsafe{_mm256_loadu_pd(out.as_ptr().add(n))};
        let ar=unsafe{_mm256_loadu_pd(a.as_ptr().add(n))};
        let bi=unsafe{_mm256_loadu_pd(b.as_ptr().add(n))};
        let next=_mm256_fmadd_pd(ar,r,_mm256_fnmadd_pd(bi,i,old));
        unsafe{_mm256_storeu_pd(out.as_mut_ptr().add(n),next);}
    }
    // Both mode-output dimensions are normally divisible by four, but the
    // portable coefficient format also admits other channel counts.
    let mut rr=[0.;4]; let mut ii=[0.;4];
    unsafe{_mm256_storeu_pd(rr.as_mut_ptr(),r);_mm256_storeu_pd(ii.as_mut_ptr(),i);}
    for n in end..out.len() { out[n]=a[n].mul_add(rr[0],(-b[n]).mul_add(ii[0],out[n])); }
}
#[inline]
fn accumulate(out:&mut[f64],a:&[f64],b:&[f64],r:f64,i:f64,simd:bool) {
    #[cfg(target_arch="x86_64")]
    if simd { unsafe{accumulate_avx(out,a,b,r,i);} return; }
    for ((y,a),b) in out.iter_mut().zip(a).zip(b) { *y=a.mul_add(r,(-b).mul_add(i,*y)); }
}
pub struct PressureBody {
    n: usize, p: usize, external: usize, groups: usize, frames: usize, dt: f64, simd: bool,
    coefficients: Vec<f64>, br: Vec<f64>, bi: Vec<f64>, factor: Vec<f64>,
    mapping: Vec<f64>, transpose: Vec<f64>, pr: Vec<f64>, pi: Vec<f64>, pf: Vec<f64>,
    delay: Vec<usize>, taps: Vec<f64>, history: Vec<f64>, cursor: usize,
    real: Vec<f64>, imag: Vec<f64>, force: Vec<f64>, velocity: Vec<f64>, group_pressure: Vec<f64>, mobility: Vec<f64>,
}
impl PressureBody {
    pub fn load(path: &str) -> Self {
        let bytes = fs::read(path).expect("pressure coefficients");
        assert!(bytes.len() >= 36 && &bytes[..8] == b"RFPRE002");
        let integer = |o| u32::from_le_bytes(bytes[o..o+4].try_into().unwrap()) as usize;
        let (n,p,external,groups,frames) = (integer(8),integer(12),integer(16),integer(20),integer(24));
        let dt = f64::from_le_bytes(bytes[28..36].try_into().unwrap());
        assert!(n>0 && n<=1212 && p>0 && p<=128 && external>0 && external<=128 && groups>0 && groups%2==0 && groups<=16 && frames>16 && frames<10000 && dt.is_finite() && dt>0.);
        assert_eq!(bytes.len(),36+8*(11*n+2*n*p+external*p+2*groups*n+groups*p+16*groups)+4*groups);
        let mut offset = 36;
        let mut read = |count| {
            let result = bytes[offset..offset+8*count].chunks_exact(8).map(|v| f64::from_le_bytes(v.try_into().unwrap())).collect::<Vec<_>>();
            offset+=8*count;
            assert!(result.iter().all(|v| v.is_finite())); result
        };
        let coefficients=read(10*n); let br=read(n*p); let bi=read(n*p); let factor=read(n);
        let mapping=read(external*p); let pr0=read(groups*n); let pi0=read(groups*n);
        let pf=read(groups*p);
        let mut pr=vec![0.;groups*n]; let mut pi=vec![0.;groups*n];
        for m in 0..n { for g in 0..groups { pr[m*groups+g]=pr0[g*n+m]; pi[m*groups+g]=pi0[g*n+m]; } }
        let delay=bytes[offset..offset+4*groups].chunks_exact(4).map(|v| u32::from_le_bytes(v.try_into().unwrap()) as usize).collect::<Vec<_>>();
        offset+=4*groups;
        assert!(delay.iter().all(|v| v+16<frames));
        let taps=bytes[offset..].chunks_exact(8).map(|v| f64::from_le_bytes(v.try_into().unwrap())).collect::<Vec<_>>();
        assert!(taps.iter().all(|v| v.is_finite()));
        let mut transpose=vec![0.; mapping.len()];
        for row in 0..external { for col in 0..p { transpose[col*external+row]=mapping[row*p+col]; } }
        #[cfg(target_arch="x86_64")]
        let simd=std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma");
        #[cfg(not(target_arch="x86_64"))]
        let simd=false;
        let mut mobility=vec![0.;p*p];
        for m in 0..n { for j in 0..p { for k in 0..p {
            mobility[j*p+k]+=factor[m]*(coefficients[m*10+6]*(br[m*p+j]*br[m*p+k]-bi[m*p+j]*bi[m*p+k])
                -coefficients[m*10+7]*(br[m*p+j]*bi[m*p+k]+bi[m*p+j]*br[m*p+k]));
        } } }
        Self { n,p,external,groups,frames,dt,simd,coefficients,br,bi,factor,mapping,transpose,pr,pi,pf,delay,taps,
            history:vec![0.;groups*frames],cursor:0,real:vec![0.;n],imag:vec![0.;n],force:vec![0.;p],velocity:vec![0.;p],group_pressure:vec![0.;groups],mobility }
    }
    pub fn sample_rate(&self)->f64 { 1./self.dt }
    pub fn latent_ports(&self)->usize { self.p }
    pub fn held_mobility(&self)->&[f64] { &self.mobility }
    /// Mean velocity before an unknown force: v = v_free + M_held * F.
    /// Does not advance states or microphone delay histories. A coupled
    /// contact solver can close its ports before accepting a single step.
    pub fn free_latent_velocity(&self,out:&mut[f64]) {
        assert_eq!(out.len(),self.p); out.fill(0.);
        for m in 0..self.n {
            let c=&self.coefficients[m*10..(m+1)*10];
            let r=self.factor[m]*(c[2]*self.real[m]-c[3]*self.imag[m]);
            let i=self.factor[m]*(c[2]*self.imag[m]+c[3]*self.real[m]);
            accumulate(out,&self.br[m*self.p..(m+1)*self.p],&self.bi[m*self.p..(m+1)*self.p],r,i,self.simd);
        }
    }
    pub fn step(&mut self, external_force: &[f64], out: &mut [f64]) -> [f64;2] {
        assert_eq!(external_force.len(),self.external); assert_eq!(out.len(),self.external);
        for (u,row) in self.force.iter_mut().zip(self.transpose.chunks_exact(self.external)) { *u=mapped_dot(row,external_force,self.simd); }
        let pressure=self.advance();
        for (y,row) in out.iter_mut().zip(self.mapping.chunks_exact(self.p)) { *y=mapped_dot(row,&self.velocity,self.simd); }
        pressure
    }
    pub fn step_latent(&mut self,force:&[f64],out:&mut[f64])->[f64;2] {
        assert_eq!(force.len(),self.p); assert_eq!(out.len(),self.p);
        self.force.copy_from_slice(force);
        let pressure=self.advance(); out.copy_from_slice(&self.velocity); pressure
    }
    fn advance(&mut self)->[f64;2] {
        self.velocity.fill(0.);
        for (g,row) in self.group_pressure.iter_mut().zip(self.pf.chunks_exact(self.p)) { *g=mapped_dot(row,&self.force,self.simd); }
        for mode in 0..self.n {
            let br=&self.br[mode*self.p..(mode+1)*self.p]; let bi=&self.bi[mode*self.p..(mode+1)*self.p];
            #[cfg(target_arch="x86_64")]
            let (ur,ui)=if self.simd { unsafe{dot_pair_avx(br,bi,&self.force)} } else { dot_pair(br,bi,&self.force) };
            #[cfg(not(target_arch="x86_64"))]
            let (ur,ui)=dot_pair(br,bi,&self.force);
            let c=&self.coefficients[mode*10..(mode+1)*10];
            let r=self.real[mode]; let i=self.imag[mode];
            let ar=self.factor[mode]*(c[2]*r-c[3]*i+c[6]*ur-c[7]*ui);
            let ai=self.factor[mode]*(c[2]*i+c[3]*r+c[6]*ui+c[7]*ur);
            let dr=c[8]*r-c[9]*i+c[4]*ur-c[5]*ui;
            let di=c[8]*i+c[9]*r+c[4]*ui+c[5]*ur;
            self.real[mode]+=dr; self.imag[mode]+=di;
            accumulate(&mut self.velocity,br,bi,ar,ai,self.simd);
            accumulate(&mut self.group_pressure,&self.pr[mode*self.groups..(mode+1)*self.groups],&self.pi[mode*self.groups..(mode+1)*self.groups],r,i,self.simd);
        }
        let mut pressure=[0.;2];
        for g in 0..self.groups {
            self.history[g*self.frames+self.cursor]=self.group_pressure[g];
            let mut result=0.;
            for tap in 0..16 {
                let index=(self.cursor+self.frames-self.delay[g]-tap)%self.frames;
                result=self.taps[g*16+tap].mul_add(self.history[g*self.frames+index],result);
            }
            pressure[g/(self.groups/2)]+=result;
        }
        self.cursor=(self.cursor+1)%self.frames;
        pressure
    }
}
fn force(frame:usize,port:usize)->f64 { if frame<400 { (((frame*17+port*13)%97) as f64-48.)/48. } else { 0. } }
fn main() {
    let args=env::args().collect::<Vec<_>>(); let mut body=PressureBody::load(&args[1]);
    if args.get(3).map(String::as_str)==Some("scalar") { body.simd=false; }
    let mut f=vec![0.;body.external]; let mut out=f.clone();
    if args.get(2).map(String::as_str)==Some("verify") {
        let mut pressure=Vec::with_capacity(1200); let mut work=0.; let mut checksum=0.; let mut port_identity_max=0f64;
        let mut free=vec![0.;body.p]; let mut predicted=vec![0.;body.p]; let mut latent=vec![0.;body.p];
        for frame in 0..1200 {
            for (p,v) in f.iter_mut().enumerate() { *v=force(frame,p); }
            body.free_latent_velocity(&mut free);
            for (u,row) in latent.iter_mut().zip(body.transpose.chunks_exact(body.external)) { *u=dot(row,&f); }
            for j in 0..body.p { predicted[j]=free[j]+dot(&body.mobility[j*body.p..(j+1)*body.p],&latent); }
            pressure.push(body.step(&f,&mut out)); work+=body.dt*dot(&f,&out);
            let difference=body.velocity.iter().zip(&predicted).map(|(a,b)|(a-b)*(a-b)).sum::<f64>().sqrt();
            let scale=body.velocity.iter().map(|a|a*a).sum::<f64>().sqrt().max(1e-30);
            port_identity_max=port_identity_max.max(difference/scale); assert!(difference/scale<=1e-8);
            checksum+=out.iter().enumerate().map(|(p,v)|(p+1) as f64*v).sum::<f64>();
        }
        println!("{{\"real\":{:?},\"imag\":{:?},\"pressure\":{:?},\"work_j\":{},\"checksum\":{},\"held_port_identity_max_relative\":{}}}",body.real,body.imag,pressure,work,checksum,port_identity_max);
        return;
    }
    let blocks=2048; let frames=256;
    let input=(0..frames*body.external).map(|i|force(i/body.external,i%body.external)).collect::<Vec<_>>();
    for _ in 0..32 { for f in input.chunks_exact(body.external) { black_box(body.step(f,&mut out)); } }
    let mut times=Vec::with_capacity(blocks); let allocations=ALLOCATIONS.load(Ordering::Relaxed);
    let mut peak=0f64;
    for _ in 0..blocks {
        let start=Instant::now();
        for f in input.chunks_exact(body.external) { let p=body.step(black_box(f),black_box(&mut out)); peak=peak.max(p[0].abs()).max(p[1].abs()); }
        times.push(start.elapsed().as_secs_f64()*1000.);
    }
    let allocations=ALLOCATIONS.load(Ordering::Relaxed)-allocations;
    let mean=times.iter().sum::<f64>()/blocks as f64; times.sort_by(f64::total_cmp);
    println!("{{\"rate_hz\":{},\"frames\":{},\"buffers\":{},\"deadline_ms\":{},\"mean_ms\":{},\"p99_ms\":{},\"max_ms\":{},\"allocations\":{},\"pressure_peak_pa\":{}}}",1./body.dt,frames,blocks,frames as f64*body.dt*1000.,mean,times[blocks*99/100],times[blocks-1],allocations,peak);
}
