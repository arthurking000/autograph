use autograph::backend::{Buffer, Device, Num};
use autograph::tensor::Tensor;
use autograph::{include_spirv, Result};
use bytemuck::{Pod, Zeroable};
use ndarray::Array;

#[test]
fn device_list() {
    Device::list();
}

#[derive(Clone, Copy, Zeroable, Pod)]
#[repr(C)]
struct FillU32PushConsts {
    n: u32,
    x: u32,
}

#[test]
fn compute_pass() -> Result<()> {
    let spirv = include_spirv!("../src/shaders/glsl/fill_f32.spv");

    for gpu in Device::list_gpus() {
        let n = 10;
        let mut y = Buffer::<u32>::zeros(&gpu, n)?;
        gpu.compute_pass(spirv.as_ref(), "main")?
            .buffer_slice_mut(y.as_buffer_slice_mut())?
            .push_constants(FillU32PushConsts { n: n as u32, x: 1 })?
            .global_size([n as u32, 1, 1])
            .enqueue()?;
        let y = smol::block_on(y.to_vec()?)?;
        assert_eq!(y, vec![1u32; n]);
    }

    Ok(())
}

#[test]
fn tensor_zeros() -> Result<()> {
    for device in Device::list() {
        Tensor::<f32, _>::zeros(&device, [64, 1, 28, 28])?;
    }
    Ok(())
}

#[test]
fn tensor_from_shape_cow() -> Result<()> {
    for device in Device::list() {
        Tensor::<f32, _>::from_shape_cow(&device, [64, 1, 28, 28], vec![1.; 64 * 1 * 28 * 28])?;
        let x = vec![1., 2., 3., 4.];
        let y = Tensor::<f32, _>::from_shape_cow(&device, x.len(), x.as_slice())?;
        let y = smol::block_on(y.to_vec()?)?;
        assert_eq!(x, y);
    }
    Ok(())
}

#[test]
fn tensor_from_array() -> Result<()> {
    for device in Device::list() {
        let x = Array::<f32, _>::from_shape_vec([2, 2], vec![1., 2., 3., 4.])?;
        let y = smol::block_on(Tensor::from_array(&device, x.view())?.to_array()?)?;
        assert_eq!(x, y);
        let y_t = smol::block_on(Tensor::from_array(&device, x.t())?.to_array()?)?;
        assert_eq!(x.t(), y_t.view());
    }
    Ok(())
}

fn tensor_from_elem<T: Num>(xs: &[T]) -> Result<()> {
    let n = 1200;
    for device in Device::list() {
        for x in xs.iter().copied() {
            let y = Tensor::from_elem(&device, n, x)?;
            let y = smol::block_on(y.to_vec()?)?;
            assert_eq!(y, vec![x; n]);
        }
    }

    Ok(())
}

#[test]
fn test_from_elem_f32() -> Result<()> {
    tensor_from_elem::<f32>(&[1., 33., 0.1, 1000.])
}

#[test]
fn test_from_elem_u32() -> Result<()> {
    tensor_from_elem::<u32>(&[1, 33, 1000])
}

#[test]
fn test_from_elem_i32() -> Result<()> {
    tensor_from_elem::<i32>(&[1, 33, 1000])
}
