//! Compiled as an example in the exact pinned external sofar checkout, never Aurora core.
use sofar::reader::{Filter, OpenOptions, Sofar};
use sofar::render::Renderer;
use std::{error::Error, fs};

const TAPS: usize = 1024;
const FRAMES: usize = 2048;

fn response(sofa: &Sofar, p: [f32; 3]) -> Result<Filter, Box<dyn Error>> {
    let mut raw = Filter::new(sofa.filter_len());
    sofa.filter(p[0], p[1], p[2], &mut raw);
    let mut padded = Filter::new(TAPS);
    for (source, delay, target) in [
        (&raw.left, raw.ldelay, &mut padded.left),
        (&raw.right, raw.rdelay, &mut padded.right),
    ] {
        if !delay.is_finite() || delay < 0.0 { return Err("invalid SOFA delay".into()); }
        let samples = delay * 48_000.0;
        let offset = samples.floor() as usize;
        let fraction = samples.fract();
        if offset + source.len() + 1 > TAPS { return Err("SOFA filter exceeds prepared bound".into()); }
        for (i, value) in source.iter().enumerate() {
            target[offset+i] += value * (1.0-fraction);
            target[offset+i+1] += value * fraction;
        }
    }
    Ok(padded)
}

// Real ACN/SN3D harmonics, Cartesian coordinates x=front, y=left, z=up.
fn sh([x,y,z]: [f32;3]) -> [f32;16] {
    let s3 = 3.0_f32.sqrt();
    let s15 = 15.0_f32.sqrt();
    let s38 = (3.0_f32/8.0).sqrt();
    let s58 = (5.0_f32/8.0).sqrt();
    [1.0,y,z,x,s3*x*y,s3*y*z,0.5*(3.0*z*z-1.0),s3*x*z,
     s3*0.5*(x*x-y*y),s58*y*(3.0*x*x-y*y),s15*x*y*z,
     s38*y*(5.0*z*z-1.0),0.5*z*(5.0*z*z-3.0),s38*x*(5.0*z*z-1.0),
     s15*0.5*z*(x*x-y*y),s58*x*(x*x-3.0*y*y)]
}

fn direction(az: f32, el: f32) -> [f32;3] {
    let (a,e) = (az.to_radians(), el.to_radians());
    [a.cos()*e.cos(), a.sin()*e.cos(), e.sin()]
}

fn rotate_to_head([x,y,z]: [f32;3], yaw: f32, pitch: f32, roll: f32) -> [f32;3] {
    let (s,c)=yaw.to_radians().sin_cos();
    let (x,y)=(c*x+s*y,-s*x+c*y);
    let (s,c)=pitch.to_radians().sin_cos();
    let (x,z)=(c*x+s*z,-s*x+c*z);
    let (s,c)=roll.to_radians().sin_cos();
    [x,c*y+s*z,-s*y+c*z]
}

fn hoa(sofa: &Sofar, order: usize, yaw: f32) -> Result<Vec<Filter>,Box<dyn Error>> {
    let mut filters: Vec<_>=(0..(order+1).pow(2)).map(|_|Filter::new(TAPS)).collect();
    // Eight-point Gauss-Legendre z quadrature and sixteen uniform azimuths.
    // Integrates degree-six SH products exactly; HRTF field integration is approximate.
    let nodes=[(-0.96028986,0.101228535),(-0.7966665,0.22238103),
        (-0.5255324,0.31370664),(-0.18343464,0.36268377),
        (0.18343464,0.36268377),(0.5255324,0.31370664),
        (0.7966665,0.22238103),(0.96028986,0.101228535)];
    for (z,w) in nodes {
        for a in 0..16 {
            let az=a as f32*std::f32::consts::TAU/16.0;
            let radius=(1.0_f32-z*z).sqrt();
            let p=[radius*az.cos(),radius*az.sin(),z];
            let h=response(sofa,rotate_to_head(p,yaw,0.0,0.0))?;
            let harmonics=sh(p);
            for (acn, target) in filters.iter_mut().enumerate() {
                let degree=(acn as f32).sqrt().floor() as usize;
                let scale=w/32.0*(2*degree+1) as f32*harmonics[acn];
                for i in 0..TAPS {
                    target.left[i]+=h.left[i]*scale;
                    target.right[i]+=h.right[i]*scale;
                }
            }
        }
    }
    Ok(filters)
}

fn case(name:&str, order:usize, filters:Vec<Filter>, weights:Vec<f32>, head:[f32;3]) -> Result<String,Box<dyn Error>> {
    let mut expected=vec![0.0;FRAMES*2];
    let mut coefficients=Vec::new();
    for (filter,weight) in filters.iter().zip(weights.iter()) {
        let mut renderer=Renderer::builder(TAPS).with_partition_len(256).build()?;
        renderer.set_filter(filter)?;
        let mut impulse=vec![0.0;FRAMES];
        impulse[256]=0.25*weight;
        let (mut left,mut right)=(vec![0.0;FRAMES],vec![0.0;FRAMES]);
        renderer.process_block(&impulse,&mut left,&mut right)?;
        for i in 0..FRAMES { expected[2*i]+=left[i]; expected[2*i+1]+=right[i]; }
        coefficients.extend_from_slice(&filter.left);
        coefficients.extend_from_slice(&filter.right);
    }
    if coefficients.iter().chain(expected.iter()).any(|x|!x.is_finite()) { return Err("nonfinite oracle".into()); }
    Ok(format!("{{\"name\":{name:?},\"hoa_order\":{order},\"head_direction\":{head:?},\"weights\":{weights:?},\"coefficients\":{coefficients:?},\"expected\":{expected:?}}}"))
}

fn main()->Result<(),Box<dyn Error>> {
    let args:Vec<_>=std::env::args().collect();
    if args.len()!=3 { return Err("usage: sofar_prepared_export <sofa> <json>".into()); }
    let sofa=OpenOptions::new().sample_rate(48_000.0).open(&args[1])?;
    let mut cases=Vec::new();
    for (name,az,el,yaw,pitch,roll) in [
        ("front",0.0,0.0,0.0,0.0,0.0),("back",180.0,0.0,0.0,0.0,0.0),
        ("left",90.0,0.0,0.0,0.0,0.0),("right",-90.0,0.0,0.0,0.0,0.0),
        ("up",0.0,45.0,0.0,0.0,0.0),("down",0.0,-45.0,0.0,0.0,0.0),
        ("yaw-left",0.0,0.0,-90.0,0.0,0.0),("yaw-right",0.0,0.0,90.0,0.0,0.0),
        ("pitch-up",0.0,0.0,0.0,-45.0,0.0),("roll-up",90.0,0.0,0.0,0.0,-45.0),
    ] {
        let p=rotate_to_head(direction(az,el),yaw,pitch,roll);
        cases.push(case(name,0,vec![response(&sofa,p)?],vec![1.0],p)?);
    }
    for order in 1..=3 {
        for (name,az,yaw) in [("left",90.0,0.0),("right",-90.0,0.0),("yaw-right",0.0,90.0)] {
            let p=direction(az,0.0);
            cases.push(case(&format!("hoa{order}-{name}"),order,hoa(&sofa,order,yaw)?,sh(p)[..(order+1).pow(2)].to_vec(),rotate_to_head(p,yaw,0.0,0.0))?);
        }
    }
    fs::write(&args[2],format!("{{\"schema_version\":1,\"sample_rate\":48000,\"taps\":{TAPS},\"frames\":{FRAMES},\"cases\":[{}]}}",cases.join(",")))?;
    Ok(())
}
