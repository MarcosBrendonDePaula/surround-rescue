//! Plugin VST2 mínimo para testes: ganho fixo de -6 dB, 2 entradas e 2 saídas, sem janela.
//! Implementa só o necessário da ABI do VST 2.4 (a mesma que o executor usa para carregar).

use std::ffi::c_void;

type HostCallback = unsafe extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;

#[repr(C)]
pub struct AEffect {
    magic: i32,
    dispatcher: unsafe extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize,
    process: *const c_void,
    set_parameter: unsafe extern "C" fn(*mut AEffect, i32, f32),
    get_parameter: unsafe extern "C" fn(*mut AEffect, i32) -> f32,
    num_programs: i32,
    num_params: i32,
    num_inputs: i32,
    num_outputs: i32,
    flags: i32,
    resvd1: isize,
    resvd2: isize,
    initial_delay: i32,
    real_qualities: i32,
    off_qualities: i32,
    io_ratio: f32,
    object: *mut c_void,
    user: *mut c_void,
    unique_id: i32,
    version: i32,
    process_replacing: unsafe extern "C" fn(*mut AEffect, *const *const f32, *mut *mut f32, i32),
    process_double_replacing: *const c_void,
    future: [u8; 56],
}

static mut GAIN: f32 = 0.5;

unsafe extern "C" fn dispatcher(e: *mut AEffect, op: i32, _i: i32, _v: isize, ptr: *mut c_void, _o: f32) -> isize {
    match op {
        1 => {
            // effClose
            drop(Box::from_raw(e));
            1
        }
        45 => {
            // effGetEffectName
            let n = b"SR Teste Ganho\0";
            std::ptr::copy_nonoverlapping(n.as_ptr(), ptr as *mut u8, n.len());
            1
        }
        _ => 0,
    }
}

unsafe extern "C" fn set_parameter(_e: *mut AEffect, i: i32, v: f32) {
    if i == 0 {
        GAIN = v;
    }
}

unsafe extern "C" fn get_parameter(_e: *mut AEffect, i: i32) -> f32 {
    if i == 0 { GAIN } else { 0.0 }
}

unsafe extern "C" fn process_replacing(_e: *mut AEffect, ins: *const *const f32, outs: *mut *mut f32, n: i32) {
    for c in 0..2 {
        let i = std::slice::from_raw_parts(*ins.add(c), n as usize);
        let o = std::slice::from_raw_parts_mut(*outs.add(c), n as usize);
        for (a, b) in o.iter_mut().zip(i) {
            *a = b * GAIN;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn VSTPluginMain(_host: HostCallback) -> *mut AEffect {
    Box::into_raw(Box::new(AEffect {
        magic: 0x5673_7450,
        dispatcher,
        process: std::ptr::null(),
        set_parameter,
        get_parameter,
        num_programs: 0,
        num_params: 1,
        num_inputs: 2,
        num_outputs: 2,
        flags: 1 << 4, // effFlagsCanReplacing
        resvd1: 0,
        resvd2: 0,
        initial_delay: 0,
        real_qualities: 0,
        off_qualities: 0,
        io_ratio: 1.0,
        object: std::ptr::null_mut(),
        user: std::ptr::null_mut(),
        unique_id: 0x5352_7447, // 'SRtG'
        version: 1,
        process_replacing,
        process_double_replacing: std::ptr::null(),
        future: [0; 56],
    }))
}
