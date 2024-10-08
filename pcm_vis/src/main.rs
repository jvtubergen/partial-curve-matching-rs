use std::{fs, io::Read, iter::zip, path::Path};
extern crate rand;
use pcm::prelude::*;
use rand::Rng;

use serde_derive::{Serialize, Deserialize};
use std::fs::File;
use std::io::Write;
use bincode;

use plotters::prelude::*;
use full_palette::{GREEN_400, RED_300};


// =============
// === Curve ===
// =============

/// Construct curve with n number of random points in f64 domain.
/// Chance of generating points which break general position is sufficiently small to ignore testing.
fn random_curve(n: usize, fieldsize: f64) -> Curve {
    let mut rng = rand::thread_rng();
    let c: Curve = (0..n).into_iter().map(|_| Vector {x: rng.gen_range(0.0..fieldsize), y: rng.gen_range(0.0..fieldsize)}).collect();
    c
}

/// Translate all points of a curve c1 by a vector q.
fn translate_curve(c1: Curve, q: Vector) -> Curve {
    c1.into_iter().map(|p| p + q).collect()
}

/// Add some random noise to curve points.
fn perturb_curve(c: Curve, deviation: f64) -> Curve {
    let mut rng = rand::thread_rng();
    let d = (1./2.0_f64.sqrt()) * deviation * deviation;
    c.into_iter().map(|p| p + d * rng.gen::<f64>() * Vector {x: 1., y: 1.} ).collect()
}

/// Compute curve length.
fn curve_length(c: &Curve) -> f64 {
    let mut length = 0.;
    for (p1, p2) in zip(c, &c[1..]) {
        length += p1.distance(*p2);
    }
    length
}


// ===========================
// === Visualization logic ===
// ===========================

/// Drawing Free-Space Diagram as an image to disk. If provided, draw steps along the RSD.
fn draw_fsd(fsd: &FSD, filename: &str, opt_steps: Option<Steps>) -> Result<(), Box<dyn std::error::Error>> {
    let margin = 20; // 20 pixels margin
    let width = fsd.n * 20 + 2 * margin;
    let height = fsd.m * 20 + 2 * margin;
    let filename = format!("{}.png", filename);
    let drawing_area = BitMapBackend::new(&filename, (width as u32, height as u32)).into_drawing_area();
    drawing_area.fill(&WHITE)?;

    let drawing_area = drawing_area.margin(20, 20, 20, 20);

    let n = fsd.n;
    let m = fsd.m;

    let unreachable = ShapeStyle {
        color: RED_300.mix(0.6),
        filled: true,
        stroke_width: 1,
    };
    let reachable = ShapeStyle {
        color: GREEN_400.mix(0.6),
        filled: true,
        stroke_width: 1,
    };
    let path = ShapeStyle {
        color: BLACK.mix(1.0),
        filled: true,
        stroke_width: 1,
    };

    let mut reachable_segments = vec![];
    let mut unreachable_segments = vec![];
    
    // Find reachable and unreachable segments.
    for j in 0..m {
        for i in 0..n {
            for axis in 0..2 {
                let (w,h) = fsd.dims[axis];
                let (x,y) = [(i,j), (j,i)][axis];
                if y < h {
                    let curr = (axis, x, y);
                    if let Some(LineBoundary { a, b }) = fsd.segs[curr] {
                        if a > 0. { unreachable_segments.push(vec![ (axis, x as f64, y as f64    ), (axis, x as f64, y as f64 + a) ]); }
                                      reachable_segments.push(vec![ (axis, x as f64, y as f64 + a), (axis, x as f64, y as f64 + b) ]);
                        if b < 1. { unreachable_segments.push(vec![ (axis, x as f64, y as f64 + b), (axis, x as f64, y as f64 + 1.) ]); }
                    } else {
                        unreachable_segments.push(vec![ (axis, x as f64, y as f64), (axis, x as f64, y as f64 + 1.) ])
                    }
                }
            }
        }
    }

    // Draw reachable and unreachable line segments.
    let height = 20*m as i32;
    // println!("reachable:");
    for seg in reachable_segments {
        // println!("{seg:?}");
        let seg: Vec<(i32, i32)> = seg.into_iter().map(|(axis, x, y)| {
            // ((20.*x) as i32, (20.*y) as i32)
            if axis == 0 { ((20.*x) as i32, height - (20.*y) as i32) }
            else         { ((20.*y) as i32, height - (20.*x) as i32) }
        }).collect();
        drawing_area.draw(&Polygon::new(seg, reachable))?;
    }

    // println!("unreachable:");
    for seg in unreachable_segments {
        // println!("{seg:?}");
        let seg: Vec<(i32, i32)> = seg.into_iter().map(|(axis, x, y)| {
            // ((20.*x) as i32, (20.*y) as i32)
            if axis == 0 { ((20.*x) as i32, height - (20.*y) as i32) }
            else         { ((20.*y) as i32, height - (20.*x) as i32) }
        }).collect();
        drawing_area.draw(&Polygon::new(seg, unreachable))?;
    }

    if let Some(steps) = opt_steps {
        for ((x1, y1), (x2, y2)) in zip(&steps,&steps[1..]) {
            let seg: Vec<(i32, i32)> = vec![((20.* x1) as i32, height - (20.*y1) as i32), ((20.* x2) as i32, height - (20.*y2) as i32)];
            drawing_area.draw(&Polygon::new(seg, path))?;
        }
    }

    Ok(())
}

fn draw_curves(c1: Curve, c2: Curve, filename: &str) -> Result<(), Box<dyn std::error::Error>> {

    // Setting up drawing area.
    let margin = 20; // 20 pixels margin
    let width = 400 + 2 * margin;
    let height = 400 + 2 * margin;
    let filename = format!("{}.png", filename);
    let drawing_area = BitMapBackend::new(&filename, (width as u32, height as u32)).into_drawing_area();
    drawing_area.fill(&WHITE)?;
    let drawing_area = drawing_area.margin(20, 20, 20, 20);

    // Computing boundaries.
    let pmin = c1.clone().into_iter().chain(c2.clone().into_iter()).reduce(|acc, v| acc.min(&v)).unwrap();
    let pmax = c1.clone().into_iter().chain(c2.clone().into_iter()).reduce(|acc, v| acc.max(&v)).unwrap();
    let pdiff = pmax - pmin;

    // Computing curve point positions on drawing area.
    let vector_to_point = |v| {
        let position = Vector::new(400., 400.) * (v - pmin) / pdiff;
        (position.x as i32, position.y as i32)
    };
    
    let seg1: Vec<(i32, i32)> = c1.into_iter().map(vector_to_point).collect();
    let seg2: Vec<(i32, i32)> = c2.into_iter().map(vector_to_point).collect();

    // Drawing the two polygonal chains.
    let colorc1 = ShapeStyle {
        color: RED_300.mix(0.6),
        filled: true,
        stroke_width: 1,
    };
    let colorc2 = ShapeStyle {
        color: GREEN_400.mix(0.6),
        filled: true,
        stroke_width: 1,
    };

    for (p1, p2) in zip(&seg1, &seg1[1..]) {
        drawing_area.draw(&Polygon::new(vec![*p1, *p2], colorc1))?;
    }

    for (p1, p2) in zip(&seg2, &seg2[1..]) {
        drawing_area.draw(&Polygon::new(vec![*p1, *p2], colorc2))?;
    }


    Ok(())
}


// =====================
// === Testing logic ===
// =====================

/// Check the presence/absence of fully covered line segments with free/unfree line segments.
fn check_corner_consistency(fsd: &FSD) -> Result<(), String> {
    let (n,m) = (fsd.n, fsd.m);
    for j in 0..m {
        for i in 0..n {
            for axis in 0..2 {
                let (w,h) = fsd.dims[axis];
                let (x,y) = [(i,j), (j,i)][axis];
                let opt_curr = if y < h { Some((axis, x, y)) } else { None };
                let opt_prev = if y > 0 { Some((axis, x, y-1)) } else { None };
                let has_corner = fsd.corners[(i, j)];

                if !has_corner {
                    if let Some(curr) = opt_curr {
                        if let Some(LineBoundary { a, b }) = fsd.segs[curr] {
                            if a == 0.0 {
                                return Err(format!("Start of boundary exists at {curr:?} while no corner at (({i},{j})) "));
                            }
                        }
                    }
                    if let Some(prev) = opt_prev {
                        if let Some(LineBoundary { a, b }) = fsd.segs[prev] {
                            if b == 1.0 {
                                return Err(format!("End of boundary exists at {prev:?} while no corner at (({i},{j})) "));
                            }
                        }
                    }
                } else { // if has_corner {
                    if let Some(curr) = opt_curr {
                        if let Some(LineBoundary { a, b }) = fsd.segs[curr] {
                            if a >= EPS {
                                return Err(format!("Start of boundary does not exist at {curr:?} while corner at (({i},{j})) "));
                            }
                        } else {
                            return Err(format!("Boundary does not exist at {curr:?} while corner at (({i},{j})) "));
                        }
                    }
                    if let Some(prev) = opt_prev {
                        if let Some(LineBoundary { a, b }) = fsd.segs[prev] {
                            if b < 1.0 - EPS {
                                return Err(format!("End of boundary does not exist at {prev:?} while corner at (({i},{j})) "));
                            }
                        } else {
                            return Err(format!("Boundary does not exist at {prev:?} while corner at (({i},{j})) "));
                        }
                    }
                } 
            }
        }
    }
    
    Ok(())
}

type Steps = Vec<(f64, f64)>;

/// Check steps result is within distance.
fn check_steps(c1: Curve, c2: Curve, steps: Vec<(f64, f64)>, eps: f64) -> Result<(), String> {

    // Check monotonic.
    for ((_i1, _j1), (_i2, _j2)) in zip(&steps, &steps[1..]) {
        if (_i1 > _i2) || (_j1 > _j2) {
             return Err(format!("Decreasing from step ({_i1}, {_j1}) to ({_i2}, {_j2})."));
        }
    }

    // Check distance while walking is within threshold.
    for (_i, _j) in steps {
        let i = _i.floor();
        let i_off = _i - i;
        let j = _j.floor();
        let j_off = _j - j;
        
        let p = if i_off == 0. { // Interpolate.
            c1[i as usize] 
        } else { (1. - i_off) * c1[i as usize] + i_off * c1[i as usize + 1] };

        let q = if j_off == 0. { // Interpolate.
            c2[j as usize] 
        } else { (1. - j_off) * c2[j as usize] + j_off * c2[j as usize + 1] };

        let d = p.distance(q);
        if !(d < eps + EPS) {
             return Err(format!("Distance {d} at step ({i}+{i_off}, {j}+{j_off}) should be below threshold {eps}+{EPS}."));
        }
    }
    Ok(())
}

/// Test validity of running a state.
// fn run_test(state: State) -> Result<(), Box<dyn std::error::Error>> {
fn run_test(state: State, testnumber: usize) -> Result<(), String> {
    let State { ps, qs, eps } = state.clone();

    draw_curves(ps.clone(), qs.clone(), format!("curve_{testnumber}").as_str());

    let fsd = FSD::new(ps.clone(), qs.clone(), eps);
    check_corner_consistency(&fsd)?;
    draw_fsd(&fsd, format!("fsd_{testnumber}").as_str(), None);

    let rsd = fsd.to_rsd();
    draw_fsd(&rsd, format!("rsd_{testnumber}").as_str(), None);
    let opt_steps = rsd.pcm_steps()?;
    draw_fsd(&rsd, format!("path_{testnumber}").as_str(), opt_steps.clone());

    let partial = rsd.check_pcm();
    println!("Is there a partial curve match?: {partial:?}.");
    if partial {
        if opt_steps.is_none() {
            return Err(format!("Should find steps if partial curve match is true."));
        }
        check_steps(ps, qs, opt_steps.unwrap(), eps)?;
    }
    
    Ok(())
}


// ========================
// === IO functionality ===
// ========================

/// Testing state for storage/retrieval.
#[derive(Serialize, Deserialize, Clone)]
struct State {
    ps: Curve,
    qs: Curve,
    eps: f64
}

/// Listing files in subfolder.
fn list_files_in_subfolder<P: AsRef<Path>>(path: P) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(path_str) = path.to_str() {
                files.push(path_str.to_string());
            }
        }
    }

    Ok(files)
}

/// Write state to testdata folder as a new testcase to debug.
fn write_new_testcase(state: State) -> Result<(), Box<dyn std::error::Error>> {
    let bin = bincode::serialize(&state)?;
    fs::create_dir("testdata"); // Folder probably already exists, then will throw error.
    let files = list_files_in_subfolder("testdata")?;
    let n = files.len();
    let file_path = Path::new("testdata").join(format!("case_{n}.bin"));
    let mut file = File::create(file_path)?;
    file.write_all(&bin)?;
    Ok(())
}

/// Read states from disk, should represent testcases previously crashed (thus to debug).
fn read_cases() -> Result<Vec<State>, Box<dyn std::error::Error>> {
    let files = list_files_in_subfolder("testdata")?;
    let mut result = vec![];
    for file in files {
        let mut file = File::open(file)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer);
        let state = bincode::deserialize(&buffer)?;
        result.push(state);
    }

    Ok(result)
}


// ==================
// === Executable ===
// ==================

const DISCOVER: bool = true;
const RUN_COUNT: usize = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {

    let cases = 
    if DISCOVER {
        (0..RUN_COUNT).map(|_| {
            let ps = random_curve(5, 2.);
            // let c2 = translate_curve(ps, Vector{ x: 3. , y: 1. });
            // let qs = perturb_curve(ps.clone(), 1.);
            // let qs = random_curve(3, 2.);
            let qs = ps.clone();
            State { ps, qs, eps: 1. }
        }).collect()
    } else {
        let mut r = read_cases()?;
        r.truncate(RUN_COUNT);
        r
    };

    for (i, case) in cases.into_iter().enumerate() {
        let res_test = run_test(case.clone(), i);
        if res_test.is_err() {
            // Print we got an error.
            println!("Test case {} failed. Error message:", i);
            println!("{:?}", res_test.unwrap_err());
            // Only write new tast case in disovery mode, 
            //   otherwise we are duplicating testcases 
            //   (writing new case we just read).
            if DISCOVER { 
                write_new_testcase(case);
            }
        }
    }

    Ok(())
}