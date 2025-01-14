#![allow(warnings)]

use anyhow::Result;
use autograph::{
    krnl::scalar::ScalarElem,
    tensor::{ScalarTensorViewD, Tensor, TensorView},
};
use dry::macro_for;
use half::{bf16, f16};
#[cfg(feature = "device")]
use krnl::buffer::Buffer;
use krnl::{buffer::Slice, device::Device, scalar::Scalar};
use krnl::{device::Features, scalar::ScalarType};
#[cfg(not(target_arch = "wasm32"))]
use libtest_mimic::{Arguments, Trial};
use ndarray::{Array, Array1, Axis, Dimension, IntoDimension, RemoveAxis};
use paste::paste;
#[cfg(not(target_arch = "wasm32"))]
use std::str::FromStr;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[cfg(all(target_arch = "wasm32", run_in_browser))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args = Arguments::from_args();
    let tests = if cfg!(feature = "device") && !cfg!(miri) {
        let devices: Vec<_> = [Device::builder().build().unwrap()]
            .into_iter()
            .chain((1..).map_while(|i| Device::builder().index(i).build().ok()))
            .collect();
        if devices.is_empty() {
            panic!("No device!");
        }
        let device_infos: Vec<_> = devices.iter().map(|x| x.info().unwrap()).collect();
        println!("devices: {device_infos:#?}");
        let krnl_device = std::env::var("KRNL_DEVICE");
        let device_index = if let Ok(krnl_device) = krnl_device.as_ref() {
            usize::from_str(krnl_device).unwrap()
        } else {
            0
        };
        println!("KRNL_DEVICE = {krnl_device:?}");
        println!("testing device {device_index}");
        let device = devices.get(device_index).unwrap();
        tests(&Device::host())
            .into_iter()
            .chain(tests(device))
            .collect()
    } else {
        tests(&Device::host()).into_iter().collect()
    };
    libtest_mimic::run(&args, tests).exit()
}

#[cfg(not(target_arch = "wasm32"))]
fn device_test(device: &Device, name: &str, f: impl Fn(&Device) + Send + Sync + 'static) -> Trial {
    let name = format!(
        "{name}_{}",
        if device.is_host() { "host" } else { "device" }
    );
    let device = device.clone();
    Trial::test(name, move || {
        f(&device);
        Ok(())
    })
}

fn features_for_scalar_size(size: usize) -> Features {
    Features::empty()
        .with_shader_int8(size == 1)
        .with_shader_int16(size == 2)
        .with_shader_int64(size == 8)
}

fn features_for_scalar(scalar_type: ScalarType) -> Features {
    features_for_scalar_size(scalar_type.size()).with_shader_float64(scalar_type == ScalarType::F64)
}

fn check_approx_eq(a: ScalarTensorViewD, b: ScalarTensorViewD, epsilon: Option<ScalarElem>) {
    use approx::assert_relative_eq;
    let scalar_type = a.scalar_type();
    if matches!(scalar_type, ScalarType::F16 | ScalarType::BF16) {
        let a = a
            .cast_into(ScalarType::F32)
            .unwrap()
            .try_into_tensor::<f32>()
            .unwrap()
            .into_array()
            .unwrap();
        let b = b
            .cast_into(ScalarType::F32)
            .unwrap()
            .try_into_tensor::<f32>()
            .unwrap()
            .into_array()
            .unwrap();
        if let Some(epsilon) = epsilon {
            let epsilon = epsilon.cast::<f32>();
            assert_relative_eq!(a, b, epsilon = epsilon, max_relative = epsilon);
        } else {
            assert_relative_eq!(a, b);
        }
    } else if scalar_type == ScalarType::F32 {
        let a = a
            .try_into_tensor_view::<f32>()
            .unwrap()
            .into_array()
            .unwrap();
        let b = b
            .try_into_tensor_view::<f32>()
            .unwrap()
            .into_array()
            .unwrap();
        assert_relative_eq!(a, b);
    } else if scalar_type == ScalarType::F64 {
        let a = a
            .try_into_tensor_view::<f64>()
            .unwrap()
            .into_array()
            .unwrap();
        let b = b
            .try_into_tensor_view::<f64>()
            .unwrap()
            .into_array()
            .unwrap();
        assert_relative_eq!(a, b);
    } else {
        check_eq(a, b);
    }
}

fn check_eq(a: ScalarTensorViewD, b: ScalarTensorViewD) {
    macro_for!($T in [u8, i8, u16, i16, f16, bf16, u32, i32, f32, u64, i64, f64] {
        if a.scalar_type() == $T::scalar_type() {
            let a = a.try_into_tensor_view::<$T>().unwrap();
            let a = a.as_array().unwrap();
            let b = b.try_into_tensor_view::<$T>().unwrap();
            let b = b.as_array().unwrap();
            assert_eq!(a, b);
            return;
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn tests(device: &Device) -> Vec<Trial> {
    tensor_tests(device)
}

#[cfg(not(target_arch = "wasm32"))]
fn tensor_tests(device: &Device) -> Vec<Trial> {
    let features = device
        .info()
        .map(|x| x.features())
        .unwrap_or(Features::empty());
    let mut tests = Vec::new();

    tests.extend([
        Trial::test("tensor_from_array0", || {
            tensor_from_array(Array::from_elem((), 1));
            Ok(())
        }),
        Trial::test("tensor_from_array1", || {
            tensor_from_array(Array::from_shape_vec(3, (1..=3).into_iter().collect()).unwrap());
            Ok(())
        }),
        Trial::test("tensor_from_array2", || {
            tensor_from_array(
                Array::from_shape_vec([2, 3], (1..=6).into_iter().collect()).unwrap(),
            );
            Ok(())
        }),
        Trial::test("tensor_from_array3", || {
            tensor_from_array(
                Array::from_shape_vec([2, 3, 4], (1..=24).into_iter().collect()).unwrap(),
            );
            Ok(())
        }),
        Trial::test("tensor_from_array4", || {
            tensor_from_array(
                Array::from_shape_vec([2, 3, 4, 5], (1..=120).into_iter().collect()).unwrap(),
            );
            Ok(())
        }),
        Trial::test("tensor_from_array4", || {
            tensor_from_array(
                Array::from_shape_vec([2, 3, 4, 5, 6], (1..=120 * 6).into_iter().collect())
                    .unwrap(),
            );
            Ok(())
        }),
        Trial::test("tensor_from_array5", || {
            tensor_from_array(
                Array::from_shape_vec([2, 3, 4, 5, 6], (1..=120 * 6).into_iter().collect())
                    .unwrap(),
            );
            Ok(())
        }),
        Trial::test("tensor_from_array6", || {
            tensor_from_array(
                Array::from_shape_vec([2, 3, 4, 5, 6, 7], (1..=120 * 6 * 7).into_iter().collect())
                    .unwrap(),
            );
            Ok(())
        }),
        Trial::test("tensor_from_arrayD", || {
            tensor_from_array(
                Array::from_shape_vec(
                    [2, 3, 4, 5, 6, 7, 8].as_ref(),
                    (1..=120 * 6 * 7 * 8).into_iter().collect(),
                )
                .unwrap(),
            );
            Ok(())
        }),
    ]);
    tests.extend(
        linalg::linalg_tests(device)
            .into_iter()
            .chain(reorder::reorder_tests(device))
            .chain(reduce::reduce_tests(device))
            .chain(ops::ops_tests(device)),
    );
    #[cfg(feature = "learn")]
    tests.extend(learn::learn_tests(device));
    tests
}

fn tensor_from_array<D: Dimension>(x: Array<u32, D>) {
    let y = TensorView::try_from(x.view()).unwrap();
    assert_eq!(x.view(), y.as_array().unwrap());
    let y_t = TensorView::try_from(x.t()).unwrap();
    assert_eq!(x.t(), y_t.as_array().unwrap());
}

mod linalg {
    use super::*;
    use approx::assert_relative_eq;
    use autograph::tensor::CowTensor;
    use ndarray::{linalg::Dot, Array2};
    use std::fmt::{self, Display};

    #[cfg(not(target_arch = "wasm32"))]
    pub fn linalg_tests(device: &Device) -> Vec<Trial> {
        let mut tests = Vec::new();
        let features = if let Some(info) = device.info() {
            info.features()
        } else {
            Features::empty()
        };
        macro_for!($T in [u8, i8, u16, i16, f16, bf16, u32, i32, f32, u64, i64, f64] {
            let scalar_type = $T::scalar_type();
            let type_name = scalar_type.name();
            let ignore = device.is_device() &&
                    !features.contains(&features_for_scalar(scalar_type));
            for n in [2, 4, 5, 8, 16, 32, 64, 128] {
                let [m, k, n] = [n; 3];
                use Transpose::*;
                for (ta, tb) in [(N, N), (T, N), (N, T), (T, T)] {
                    let name = format!("tensor_dot_{type_name}_m{m}_k{k}_n{n}_{ta}{tb}");
                    tests.push(device_test(device, &name, move |device| {
                        tensor_dot::<$T>(device, [m, k, n], [ta, tb])
                    }).with_ignored_flag(ignore));
                }
            }
        });
        tests
    }

    fn gen_array<T: Scalar>(dim: [usize; 2]) -> Array2<T> {
        let n = dim[0] * dim[1];
        let vec: Vec<T> = (1..10)
            .cycle()
            .map(|x| {
                if std::mem::size_of::<T>() == 1 {
                    T::from_u8((x == 1) as u8).unwrap()
                } else {
                    T::from_usize(x).unwrap()
                }
            })
            .take(n)
            .collect();
        Array2::from_shape_vec(dim, vec).unwrap()
    }

    #[allow(unused)]
    #[derive(Clone, Copy, Debug)]
    pub enum Transpose {
        N,
        T,
    }

    impl Display for Transpose {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            let c = match self {
                Self::N => 'n',
                Self::T => 't',
            };
            write!(f, "{c}")
        }
    }

    pub fn tensor_dot<T: Scalar>(
        device: &Device,
        [m, k, n]: [usize; 3],
        [a_t, b_t]: [Transpose; 2],
    ) {
        let dim1 = match a_t {
            Transpose::N => [m, k],
            Transpose::T => [k, m],
        };
        let dim2 = match b_t {
            Transpose::N => [k, n],
            Transpose::T => [n, k],
        };
        let a1 = gen_array::<T>(dim1);
        let t1 = CowTensor::from(a1.view())
            .into_device(device.clone())
            .unwrap();
        let (a1, t1) = match a_t {
            Transpose::N => (a1.view(), t1.view()),
            Transpose::T => (a1.t(), t1.t()),
        };
        let a2 = gen_array::<T>(dim2);
        let t2 = CowTensor::from(a2.view())
            .into_device(device.clone())
            .unwrap();
        let (a2, t2) = match b_t {
            Transpose::N => (a2.view(), t2.view()),
            Transpose::T => (a2.t(), t2.t()),
        };
        let a_true = a1.dot(&a2);
        let a_out = t1.dot(&t2).unwrap().into_array().unwrap();
        let scalar_type = T::scalar_type();
        if matches!(scalar_type, ScalarType::F16 | ScalarType::BF16) {
            let a_true = a_true.map(|x| x.to_f32().unwrap());
            let a_out = a_out.map(|x| x.to_f32().unwrap());
            let epsilon = k as f32;
            assert_relative_eq!(a_true, a_out, epsilon = epsilon);
        } else if scalar_type == ScalarType::F32 {
            let a_true = a_true.map(|x| x.to_f32().unwrap());
            let a_out = a_out.map(|x| x.to_f32().unwrap());
            assert_relative_eq!(a_true, a_out);
        } else if scalar_type == ScalarType::F64 {
            let a_true = a_true.map(|x| x.to_f64().unwrap());
            let a_out = a_out.map(|x| x.to_f64().unwrap());
            assert_relative_eq!(a_true, a_out);
        } else {
            assert_eq!(a_out, a_true);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod ops {
    use super::*;
    use ndarray::{Array1, IntoDimension};
    use num_traits::Unsigned;

    pub fn ops_tests(device: &Device) -> Vec<Trial> {
        let mut tests = Vec::new();
        let features = if let Some(info) = device.info() {
            info.features()
        } else {
            Features::empty()
        };
        macro_for!($T in [u8, i8, u16, i16, f16, bf16, u32, i32, f32, u64, i64, f64] {
            let scalar_type = $T::scalar_type();
            let ignore = device.is_device() &&
                !features.contains(&features_for_scalar(scalar_type));
            let ty = scalar_type.name();
            let lens = [7, 64, 300];
            tests.push(
                device_test(device, &format!("scaled_add_{ty}"), |device| {
                    for n in [7, 64, 300] {
                        scaled_add::<$T>(device, &[n]);
                    }
                    scaled_add::<$T>(device, &[3, 5]);
                    scaled_add::<$T>(device, &[21, 14]);
                }).with_ignored_flag(ignore)
            );
        });
        macro_for!($X in [u8, u16, u32, u64] {
            let x_ty = $X::scalar_type();
            macro_for!($Y in [u8, i8, u16, i16, f16, bf16, u32, i32, f32, u64, i64, f64] {
                let y_ty = $Y::scalar_type();
                let ignore = device.is_device()
                && (!features.contains(&features_for_scalar(x_ty)) ||
                    !features.contains(&features_for_scalar(y_ty)));
                tests.push(device_test(device, &format!("one_hot_{}_{}", x_ty.name(), y_ty.name()), |device| {
                    for n in [1, 7, 64, 300] {
                        for classes in [1, 5, 10, 100] {
                            one_hot::<$X, $Y>(device, &[n], classes);
                        }
                    }
                }).with_ignored_flag(ignore));
            });
        });
        tests
    }

    fn scaled_add<T: Scalar>(device: &Device, shape: &[usize]) {
        let alpha = T::from_u32(2).unwrap();
        let shape = shape.into_dimension();
        let x_array = (1..10)
            .cycle()
            .take(shape.size())
            .map(|x| T::from_usize(x).unwrap())
            .collect::<Array1<_>>()
            .into_shape(shape.clone())
            .unwrap();
        let mut y_array = (11..20)
            .cycle()
            .take(x_array.len())
            .map(|x| T::from_usize(x).unwrap())
            .collect::<Array1<_>>()
            .into_shape(shape.clone())
            .unwrap();
        let x = Tensor::from(x_array.clone())
            .into_device(device.clone())
            .unwrap();
        let mut y = Tensor::from(y_array.clone())
            .into_device(device.clone())
            .unwrap();
        y_array.scaled_add(alpha, &x_array);
        y.scaled_add(alpha, &x).unwrap();
        let y = y.into_array().unwrap();
        assert_eq!(y, y_array);
    }

    fn one_hot<X: Scalar + Unsigned, Y: Scalar>(device: &Device, shape: &[usize], classes: usize) {
        let dim = shape.into_dimension();
        let x_array = (0..classes)
            .cycle()
            .take(dim.size())
            .map(|x| X::from_usize(x).unwrap())
            .collect::<Array1<_>>()
            .into_shape(shape)
            .unwrap();
        let mut y_shape: Vec<_> = shape.iter().copied().chain([classes]).collect();
        let mut y_array = x_array
            .iter()
            .copied()
            .flat_map(|x| {
                (0..classes)
                    .into_iter()
                    .map(move |i| Y::from_u32((i == x.to_usize().unwrap()) as u32).unwrap())
            })
            .collect::<Array<Y, _>>()
            .into_shape(y_shape.as_slice())
            .unwrap();
        let x = Tensor::from(x_array).into_device(device.clone()).unwrap();
        let y = x.to_one_hot::<Y>(classes).unwrap().into_array().unwrap();
        assert_eq!(y, y_array);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod reorder {
    use super::*;
    use ndarray::IntoDimension;

    pub fn reorder_tests(device: &Device) -> Vec<Trial> {
        let mut tests = Vec::new();

        let features = if let Some(info) = device.info() {
            info.features()
        } else {
            Features::empty()
        };
        macro_for!($T in [u8, i8, u16, i16, f16, bf16, u32, i32, f32, u64, i64, f64] {
                let scalar_type = $T::scalar_type();
                let ignore = device.is_device() &&
                    !features.contains(&features_for_scalar(scalar_type));
                let ty = scalar_type.name();
                tests.extend([
                    device_test(device, &format!("into_standard_layout2_{ty}"), |device| {
                        into_standard_layout::<$T, _>(device, [3, 3], [1, 0]);
                        into_standard_layout::<$T, _>(device, [21, 30], [1, 0]);
                    }),
                    device_test(device, &format!("into_standard_layout3_{ty}"), |device| {
                        into_standard_layout::<$T, _>(device, [1, 2, 3], [0, 2, 1]);
                        into_standard_layout::<$T, _>(device, [2, 21, 3], [1, 2, 0]);
                    }),
                    device_test(device, &format!("into_standard_layout4_{ty}"), |device| {
                        into_standard_layout::<$T, _>(device, [1, 2, 3, 3], [0, 2, 3, 1]);
                        into_standard_layout::<$T, _>(device, [2, 21, 3, 30], [0, 3, 1, 2]);
                    }),
                    device_test(device, &format!("into_standard_layout5_{ty}"), |device| {
                        into_standard_layout::<$T, _>(device, [1, 2, 3, 3, 3], [0, 2, 3, 4, 1]);
                        into_standard_layout::<$T, _>(device, [2, 17, 3, 10, 3], [0, 3, 1, 2, 4]);
                    }),
                    device_test(device, &format!("into_standard_layout6_{ty}"), |device| {
                        into_standard_layout::<$T, _>(device, [1, 2, 3, 3, 1, 3], [0, 2, 3, 4, 5, 1]);
                        into_standard_layout::<$T, _>(device, [2, 17, 3, 10, 2, 3], [0, 3, 1, 2, 5, 4]);
                    }),
                ].into_iter().map(|trial| trial.with_ignored_flag(ignore)));
        });

        tests
    }

    fn into_standard_layout<T: Scalar, E: IntoDimension>(device: &Device, shape: E, axes: E) {
        let shape = shape.into_dimension();
        let x_vec = (1..100)
            .cycle()
            .take(shape.size())
            .map(|x| T::from_usize(x).unwrap())
            .collect();
        let x_array = Array::from_shape_vec(shape, x_vec).unwrap();
        let axes = E::Dim::from_dimension(&axes.into_dimension()).unwrap();
        let y_array = x_array
            .view()
            .permuted_axes(axes.clone())
            .as_standard_layout()
            .to_owned();
        let x = Tensor::from(x_array.clone())
            .into_device(device.clone())
            .unwrap();
        let y = x
            .permuted_axes(axes)
            .into_standard_layout()
            .unwrap()
            .into_array()
            .unwrap();
        assert_eq!(y, y_array);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod reduce {
    use super::*;
    use std::mem::size_of;

    pub fn reduce_tests(device: &Device) -> Vec<Trial> {
        let mut tests = Vec::new();
        let features = device
            .info()
            .map(|info| info.features())
            .unwrap_or(Features::empty());
        macro_for!($T in [u8, i8, u16, i16, f16, bf16, u32, i32, f32, u64, i64, f64] {
            let scalar_type = $T::scalar_type();
            let ignore = device.is_device() &&
                !features.contains(&features_for_scalar(scalar_type));
            let ty_name = scalar_type.name();
            let size = size_of::<$T>();
            let ns: &[usize] = if size == 1 {
                &[4, 11]
            } else if size == 2 {
                &[4, 11, 33, 517]
            } else {
                &[4, 11, 33, 517, 1021]
            };
            tests.extend([
                device_test(device, &format!("sum_{ty_name}"), |device| {
                    for n in ns.iter().copied() {
                        sum::<$T, _>(device, n);
                    }
                    for ndim in 0 ..= 6 {
                        sum::<$T, _>(device, vec![2; ndim]);
                    }
                }).with_ignored_flag(ignore),
                device_test(device, &format!("sum_axis1_{ty_name}"), |device| {
                    for n in ns.iter().copied() {
                        sum_axis::<$T, _>(device, [n], Axis(0));
                    }
                }).with_ignored_flag(ignore),
                device_test(device, &format!("sum_axis2_{ty_name}"), |device| {
                    for n in ns.iter().copied() {
                        for axis in 0..2 {
                            let mut shape = [3; 2];
                            shape[axis] = n;
                            sum_axis::<$T, _>(device, shape, Axis(axis));
                        }
                    }
                }).with_ignored_flag(ignore),
                device_test(device, &format!("sum_axis3_{ty_name}"), |device| {
                    for n in ns.iter().copied() {
                        for axis in 0 .. 3  {
                            let mut shape = [3; 3];
                            shape[axis] = n;
                            sum_axis::<$T, _>(device, shape, Axis(axis));
                        }
                    }
                }).with_ignored_flag(ignore),
                device_test(device, &format!("sum_axis4_{ty_name}"), |device| {
                    for n in ns.iter().copied() {
                        for axis in 0 .. 4 {
                            let mut shape = [3; 4];
                            shape[axis] = n;
                            sum_axis::<$T, _>(device, shape, Axis(axis));
                        }
                    }
                }).with_ignored_flag(ignore),
                device_test(device, &format!("sum_axis5_{ty_name}"), |device| {
                    for n in ns.iter().copied() {
                        for axis in 0 .. 5 {
                            let mut shape = [3; 5];
                            shape[axis] = n;
                            sum_axis::<$T, _>(device, shape, Axis(axis));
                        }
                    }
                }).with_ignored_flag(ignore),
                device_test(device, &format!("sum_axis6_{ty_name}"), |device| {
                    for n in ns.iter().copied() {
                        for axis in 0 .. 6 {
                            let mut shape = [3; 6];
                            shape[axis] = n;
                            sum_axis::<$T, _>(device, shape, Axis(axis));
                        }
                    }
                }).with_ignored_flag(ignore),
            ]);
        });
        tests
    }

    fn sum<T: Scalar, E: IntoDimension>(device: &Device, shape: E) {
        let shape = shape.into_dimension();
        let x_array = (1..10)
            .cycle()
            .take(shape.size())
            .map(|x| {
                let size = size_of::<T>();
                let x = if size == 1 { (x == 1) as usize } else { x };
                T::from_usize(x).unwrap()
            })
            .collect::<Array1<_>>()
            .into_shape(shape.clone())
            .unwrap();
        let y_array = x_array.sum();
        let x = Tensor::from(x_array).into_device(device.clone()).unwrap();
        let y = x.sum().unwrap();
        let y = Tensor::from(vec![y]).into_shape(()).unwrap().into_dyn();
        let y_array = Tensor::from(vec![y_array])
            .into_shape(())
            .unwrap()
            .into_dyn();
        let epsilon = if matches!(T::scalar_type(), ScalarType::F16 | ScalarType::BF16) {
            Some(ScalarElem::F32(shape.size() as f32))
        } else {
            None
        };
        check_approx_eq(y.view().into(), y_array.view().into(), epsilon);
    }

    fn sum_axis<T: Scalar, E: IntoDimension>(device: &Device, shape: E, axis: Axis)
    where
        E::Dim: RemoveAxis,
    {
        let shape = shape.into_dimension();
        let x_array = (1..16)
            .cycle()
            .take(shape.size())
            .map(|x| {
                let size = size_of::<T>();
                let x = if size == 1 { (x == 1) as usize } else { x };
                T::from_usize(x).unwrap()
            })
            .collect::<Array1<_>>()
            .into_shape(shape.clone())
            .unwrap();
        let y_array = x_array.sum_axis(axis);
        let x = Tensor::from(x_array).into_device(device.clone()).unwrap();
        let y_array = Tensor::from(y_array).into_dyn();
        let y = x
            .sum_axis(axis)
            .unwrap()
            .into_device(Device::host())
            .unwrap()
            .into_dyn();
        let epsilon = if matches!(T::scalar_type(), ScalarType::F16 | ScalarType::BF16) {
            Some(ScalarElem::F32(shape[axis.0] as f32))
        } else {
            None
        };
        check_approx_eq(y.view().into(), y_array.view().into(), epsilon);
    }
}

#[cfg(feature = "learn")]
mod learn {
    use super::*;
    use approx::assert_relative_eq;
    use autograph::learn::criterion::CrossEntropyLoss;

    pub fn learn_tests(device: &Device) -> Vec<Trial> {
        let mut tests = Vec::new();
        tests.extend(criterion::criterion_tests(device));
        #[cfg(feature = "neural-network")]
        {
            tests.extend(neural_network::neural_network_tests(device));
        }
        tests
    }

    mod criterion {
        use super::*;
        use autograph::learn::criterion::Accuracy;
        use num_traits::{Float, Unsigned};

        pub fn criterion_tests(device: &Device) -> Vec<Trial> {
            let mut tests = Vec::new();
            let features = device
                .info()
                .map(|info| info.features())
                .unwrap_or(Features::empty());
            macro_for!($X in [bf16, f32] {
                macro_for!($T in [u8, u16, u32] {
                    let ignore = device.is_device()
                        && (
                            !features.contains(&features_for_scalar($X::scalar_type()))
                            || !features.contains(&features_for_scalar($T::scalar_type()))
                        );
                    tests.push(device_test(device, &format!("accuracy_{}_{}", $X::scalar_type().name(), $T::scalar_type().name()), |device| {
                        for (batch_size, classes) in [
                            (1, 8),
                            (31, 16),
                            (1000, 100),
                        ] {
                            accuracy::<$X, $T>(&device, batch_size, classes);
                        }
                    }).with_ignored_flag(ignore));
                });
            });
            macro_for!($X in [bf16, f32] {
                macro_for!($T in [u8, u16, u32] {
                    let ignore = device.is_device()
                        && (
                            !features.contains(&features_for_scalar($X::scalar_type()))
                            || !features.contains(&features_for_scalar($T::scalar_type()))
                        );
                    tests.push(device_test(device, &format!("cross_entropy_loss_{}_{}", $X::scalar_type().name(), $T::scalar_type().name()), |device| {
                        for (batch_size, classes) in [
                            (1, 8),
                            (31, 16),
                            (1000, 100),
                        ] {
                            cross_entropy_loss::<$X, $T>(&device, batch_size, classes);
                        }
                    }).with_ignored_flag(ignore));
                });
            });
            tests
        }

        fn accuracy<X: Scalar + Float, T: Scalar + Unsigned>(
            device: &Device,
            batch_size: usize,
            classes: usize,
        ) {
            let x_vec: Vec<X> = (0..classes)
                .map(|x| X::from_usize(x).unwrap())
                .cycle()
                .skip(classes / 2 + 1)
                .take(batch_size * classes)
                .collect();
            let t_vec: Vec<T> = (0..classes)
                .cycle()
                .map(|t| T::from_usize(t).unwrap())
                .take(batch_size)
                .collect();
            let x_array = Array::from(x_vec)
                .into_shape([batch_size, classes])
                .unwrap();
            let t_array = Array::from(t_vec);
            let x_host = Tensor::from(x_array);
            let t_host = Tensor::from(t_array);
            let x_device = x_host.to_device(device.clone()).unwrap();
            let t_device = t_host.to_device(device.clone()).unwrap();
            let y_host = x_host.accuracy(t_host).unwrap();
            let y_device = x_device.accuracy(t_device).unwrap();
            assert_eq!(y_host, y_device);
        }

        fn cross_entropy_loss<X: Scalar + Float, T: Scalar + Unsigned>(
            device: &Device,
            batch_size: usize,
            classes: usize,
        ) {
            let x_vec: Vec<X> = (0..10u8)
                .map(|x| X::from_u8(x).unwrap())
                .cycle()
                .take(batch_size * classes)
                .collect();
            let t_vec: Vec<T> = (0..classes)
                .cycle()
                .map(|t| T::from_usize(t).unwrap())
                .take(batch_size)
                .collect();
            let x_array = Array::from(x_vec)
                .into_shape([batch_size, classes])
                .unwrap();
            let t_array = Array::from(t_vec);
            let x_host = Tensor::from(x_array);
            let t_host = Tensor::from(t_array);
            let x_device = x_host.to_device(device.clone()).unwrap();
            let t_device = t_host.to_device(device.clone()).unwrap();
            let y_host = x_host.cross_entropy_loss(t_host).unwrap();
            let y_device = x_device.cross_entropy_loss(t_device).unwrap();
            let epsilon = if X::scalar_type() == ScalarType::BF16 {
                batch_size as f32 * 0.001
            } else {
                batch_size as f32 * f32::EPSILON
            };
            assert_relative_eq!(y_host, y_device, epsilon = epsilon, max_relative = epsilon);
        }
    }

    #[cfg(feature = "neural-network")]
    mod neural_network {
        use super::*;
        use autograph::{
            learn::neural_network::{
                autograd::Variable,
                layer::{Forward, MaxPool2, Relu},
            },
            ops::{Col2ImConv2, Col2ImConv2Options, Im2ColConv2, Im2ColConv2Options},
            tensor::Tensor1,
        };
        use num_traits::{Float, Unsigned};

        pub fn neural_network_tests(device: &Device) -> Vec<Trial> {
            let mut tests = Vec::new();
            let features = device
                .info()
                .map(|info| info.features())
                .unwrap_or(Features::empty());

            macro_for!($X in [bf16, f32] {
                macro_for!($T in [u8, u16, u32] {
                    let ignore = device.is_device()
                    && (
                        !features.contains(&features_for_scalar($X::scalar_type()))
                        || !features.contains(&features_for_scalar($T::scalar_type()))
                    );
                    tests.push(device_test(device, &format!("cross_entropy_loss_backward_{}_{}", $X::scalar_type().name(), $T::scalar_type().name()), |device| {
                        for (batch_size, classes) in [
                            (1, 8),
                            (31, 16),
                            (1000, 100),
                        ] {
                            cross_entropy_loss_backward::<$X, $T>(device, batch_size, classes);
                        }
                    }).with_ignored_flag(ignore));
                });
            });
            macro_for!($T in [bf16, f32] {
                let ignore = device.is_device()
                && !features.contains(&features_for_scalar($T::scalar_type()));
                let input_shapes = [
                    [1, 1, 5, 5],
                    [1, 1, 12, 12],
                    [2, 3, 5, 5],
                    [1, 1, 24, 24],
                ];
                tests.extend([
                    device_test(device, &format!("im2col_conv2_{}", $T::scalar_type().name()), move |device| {
                        let options = Im2ColConv2Options {
                            filter: [5, 5],
                            .. Default::default()
                        };
                        for input_shape in input_shapes {
                            im2col_conv2::<$T>(device, input_shape, &options);
                        }
                    }).with_ignored_flag(ignore),
                    device_test(device, &format!("col2im_conv2_{}", $T::scalar_type().name()), move |device| {
                        let options = Im2ColConv2Options {
                            filter: [5, 5],
                            .. Default::default()
                        };
                        for input_shape in input_shapes {
                            col2im_conv2::<$T>(device, input_shape, &options);
                        }
                    }).with_ignored_flag(ignore),
                ]);
            });
            macro_for!($T in [bf16, f32] {
                let ignore = device.is_device()
                && !features.contains(&features_for_scalar($T::scalar_type()));
                let input_shapes = [
                    [1, 1, 4, 4],
                    [1, 1, 12, 12],
                    [2, 3, 4, 4],
                    [1, 1, 24, 24],
                ];
                tests.extend([
                    device_test(device, &format!("max_pool2_{}", $T::scalar_type().name()), move |device| {
                        let pool = MaxPool2::builder().filter([2, 2]).build();
                        for input_shape in input_shapes {
                            max_pool2::<$T>(device, input_shape, &pool);
                        }
                    }).with_ignored_flag(ignore),
                    device_test(device, &format!("max_pool2_backward_{}", $T::scalar_type().name()), move |device| {
                        let pool = MaxPool2::builder().filter([2, 2]).build();
                        for input_shape in input_shapes {
                            max_pool2_backward::<$T>(device, input_shape, &pool);
                        }
                    }).with_ignored_flag(ignore),
                ]);
            });
            macro_for!($T in [bf16, f32] {
                let ignore = device.is_device()
                && !features.contains(&features_for_scalar($T::scalar_type()));
                let input_shapes = [[1, 8], [15, 20]];
                tests.extend([
                    device_test(device, &format!("relu_{}", $T::scalar_type().name()), move |device| {
                        for input_shape in input_shapes {
                            relu::<$T>(device, input_shape);
                        }
                    }).with_ignored_flag(ignore),
                    device_test(device, &format!("relu_backward_{}", $T::scalar_type().name()), move |device| {
                        for input_shape in input_shapes {
                            relu_backward::<$T>(device, input_shape);
                        }
                    }).with_ignored_flag(ignore),
                ]);
            });
            tests.extend([device_test(device, "broadcast", move |device| {
                broadcast(device, [2], [4, 2]);
                broadcast(device, [2], [4, 3, 2]);
                broadcast(device, [2], [5, 4, 3, 2]);
                broadcast(device, [2], [6, 5, 4, 3, 2]);
                broadcast(device, [2], [7, 6, 5, 4, 3, 2]);
                broadcast(device, [3, 2], [5, 4, 3, 2]);
                broadcast(device, [4, 1, 1, 3], [4, 2, 1, 3]);
            })]);
            tests
        }

        fn cross_entropy_loss_backward<X: Scalar + Float, T: Scalar + Unsigned>(
            device: &Device,
            batch_size: usize,
            classes: usize,
        ) {
            use autograph::learn::neural_network::criterion::cross_entropy_loss_backward as backward;
            let x_vec: Vec<X> = (0..10u8)
                .map(|x| X::from_u8(x).unwrap())
                .cycle()
                .take(batch_size * classes)
                .collect();
            let t_vec: Vec<T> = (0..classes)
                .cycle()
                .map(|t| T::from_usize(t).unwrap())
                .take(batch_size)
                .collect();
            let x_array = Array::from(x_vec)
                .into_shape([batch_size, classes])
                .unwrap();
            let t_array = Array::from(t_vec);
            let x_host = Tensor::from(x_array);
            let t_host = Tensor::from(t_array);
            let x_device = x_host.to_device(device.clone()).unwrap();
            let t_device = t_host.to_device(device.clone()).unwrap();
            let dy = 1f32;
            let dx_host = backward(x_host.view(), t_host.view(), dy)
                .unwrap()
                .into_dyn();
            let dx_device = backward(x_device.view(), t_device.view(), dy)
                .unwrap()
                .into_device(Device::host())
                .unwrap()
                .into_dyn();
            check_approx_eq(dx_host.view().into(), dx_device.view().into(), None);
        }

        fn im2col_conv2<T: Scalar>(
            device: &Device,
            input_shape: [usize; 4],
            options: &Im2ColConv2Options,
        ) {
            let len = input_shape.iter().product();
            let x_vec: Vec<T> = (1..=len).map(|x| T::from_usize(x).unwrap()).collect();
            let x_array = Array::from(x_vec).into_shape(input_shape).unwrap();
            let x_host = Tensor::from(x_array);
            let x_device = x_host.to_device(device.clone()).unwrap();
            let y_host = x_host.im2col_conv2(options).unwrap();
            let y_device = x_device.im2col_conv2(options).unwrap();
            assert_eq!(y_host.into_array().unwrap(), y_device.into_array().unwrap());
        }

        fn col2im_conv2<T: Scalar>(
            device: &Device,
            input_shape: [usize; 4],
            options: &Im2ColConv2Options,
        ) {
            let [batch_size, channels, ih, iw] = input_shape;
            let len = input_shape.iter().product();
            let x_vec: Vec<T> = (1..=len).map(|x| T::from_usize(x).unwrap()).collect();
            let x_array = Array::from(x_vec).into_shape(input_shape).unwrap();
            let x_host = Tensor::from(x_array);
            let y_host = x_host.im2col_conv2(options).unwrap();
            let [oh, ow] = options.output_shape([ih, iw]);
            let col2im_options = Col2ImConv2Options {
                shape: [oh, ow],
                filter: options.filter,
                padding: options.padding,
                stride: options.stride,
                dilation: options.dilation,
            };
            let dy_vec: Vec<T> = (1..=y_host.len())
                .map(|x| T::from_usize(x).unwrap())
                .collect();
            let dy_array = Array::from(dy_vec).into_shape(y_host.raw_dim()).unwrap();
            let dy_host = Tensor::from(dy_array);
            let dy_device = dy_host.to_device(device.clone()).unwrap();
            let dx_host = dy_host.col2im_conv2(&col2im_options).unwrap();
            let dx_device = dy_device.col2im_conv2(&col2im_options).unwrap();
            let [fh, fw] = options.filter;
            let epsilon = if T::scalar_type() == ScalarType::BF16 {
                Some(ScalarElem::F32((fh * fw) as f32))
            } else {
                None
            };
            check_approx_eq(
                dx_host.view().into_dyn().into(),
                dx_device.view().into_dyn().into(),
                epsilon,
            );
        }

        fn max_pool2<T: Scalar>(device: &Device, input_shape: [usize; 4], pool: &MaxPool2) {
            let len = input_shape.iter().product();
            let x_vec: Vec<T> = (0..10u8)
                .map(|x| T::from_u8(x).unwrap())
                .cycle()
                .take(len)
                .collect();
            let x_array = Array::from(x_vec).into_shape(input_shape).unwrap();
            let x_host = Tensor::from(x_array);
            let x_device = x_host.to_device(device.clone()).unwrap();
            let y_host = pool
                .forward(Variable::from(x_host))
                .unwrap()
                .into_value()
                .into_owned()
                .unwrap()
                .try_into_tensor::<T>()
                .unwrap();
            let y_device = pool
                .forward(Variable::from(x_device))
                .unwrap()
                .into_value()
                .into_owned()
                .unwrap()
                .try_into_tensor::<T>()
                .unwrap();
            assert_eq!(y_host.into_array().unwrap(), y_device.into_array().unwrap());
        }

        fn max_pool2_backward<T: Scalar>(
            device: &Device,
            input_shape: [usize; 4],
            pool: &MaxPool2,
        ) {
            let len = input_shape.iter().product();
            let x_vec: Vec<T> = (0..10u8)
                .map(|x| T::from_u8(x).unwrap())
                .cycle()
                .take(len)
                .collect();
            let x_array = Array::from(x_vec).into_shape(input_shape).unwrap();
            let x_host = Tensor::from(x_array).into_shared().unwrap();
            let x_device = x_host.to_device(device.clone()).unwrap();
            let y_host = pool
                .forward(Variable::from(x_host.clone()))
                .unwrap()
                .into_value()
                .into_owned()
                .unwrap()
                .try_into_tensor::<T>()
                .unwrap();
            let dy_vec: Vec<T> = (0..y_host.len())
                .map(|x| T::from_usize(x).unwrap())
                .collect();
            let dy_array = Array::from(dy_vec).into_shape(y_host.raw_dim()).unwrap();
            let dy_host = Tensor::from(dy_array).into_shared().unwrap();
            let x_device = x_host.to_device_shared(device.clone()).unwrap();
            let dy_device = dy_host.to_device_shared(device.clone()).unwrap();
            let dx_host = pool
                .backward(x_host.into(), dy_host.into())
                .unwrap()
                .into_owned()
                .unwrap()
                .try_into_tensor::<T>()
                .unwrap();
            let dx_device = pool
                .backward(x_device.into(), dy_device.into())
                .unwrap()
                .into_owned()
                .unwrap()
                .try_into_tensor::<T>()
                .unwrap();
            assert_eq!(
                dx_host.into_array().unwrap(),
                dx_device.into_array().unwrap()
            );
        }

        fn relu<T: Scalar>(device: &Device, input_shape: [usize; 2]) {
            let len = input_shape.iter().product();
            let x_vec: Vec<T> = (-10i8..10)
                .map(|x| T::from_i8(x).unwrap())
                .cycle()
                .take(len)
                .collect();
            let x_array = Array::from(x_vec).into_shape(input_shape).unwrap();
            let x_host = Tensor::from(x_array);
            let x_device = x_host.to_device(device.clone()).unwrap();
            let y_host = Relu
                .forward(Variable::from(x_host))
                .unwrap()
                .into_value()
                .into_owned()
                .unwrap()
                .try_into_tensor::<T>()
                .unwrap();
            let y_device = Relu
                .forward(Variable::from(x_device))
                .unwrap()
                .into_value()
                .into_owned()
                .unwrap()
                .try_into_tensor::<T>()
                .unwrap();
            assert_eq!(y_host.into_array().unwrap(), y_device.into_array().unwrap());
        }

        fn relu_backward<T: Scalar>(device: &Device, input_shape: [usize; 2]) {
            let len = input_shape.iter().product();
            let y_vec: Vec<T> = (-1i8..1)
                .map(|x| T::from_i8(x).unwrap())
                .cycle()
                .take(len)
                .collect();
            let dy_vec: Vec<T> = (0..len).map(|x| T::from_usize(x).unwrap()).collect();
            let y_array = Array::from(y_vec).into_shape(input_shape).unwrap();
            let dy_array = Array::from(dy_vec).into_shape(input_shape).unwrap();
            let y_host = Tensor::from(y_array).into_shared().unwrap();
            let dy_host = Tensor::from(dy_array).into_shared().unwrap();
            let y_device = y_host.to_device_shared(device.clone()).unwrap();
            let dy_device = dy_host.to_device_shared(device.clone()).unwrap();
            for (dy_host, dy_device) in [
                (dy_host.clone(), dy_device.clone()), // relu_backward
                (dy_host, dy_device),                 // relu_backward_mut
            ] {
                let dx_host = Relu
                    .backward(y_host.clone().into(), dy_host.into())
                    .unwrap()
                    .into_owned()
                    .unwrap()
                    .try_into_tensor::<T>()
                    .unwrap();
                let dx_device = Relu
                    .backward(y_device.clone().into(), dy_device.into())
                    .unwrap()
                    .into_owned()
                    .unwrap()
                    .try_into_tensor::<T>()
                    .unwrap();
                assert_eq!(
                    dx_host.into_array().unwrap(),
                    dx_device.into_array().unwrap()
                );
            }
        }

        fn broadcast<D1: IntoDimension + 'static, D2: IntoDimension + 'static>(
            device: &Device,
            input_dim: D1,
            output_dim: D2,
        ) {
            use autograph::tensor::ScalarArcTensor;

            let input_dim = input_dim.into_dimension();
            let output_dim = output_dim.into_dimension();
            let x = ScalarArcTensor::zeros(device.clone(), input_dim, ScalarType::F32).unwrap();
            let y = x.broadcast_shared(output_dim.clone());
            let x_var = Variable::builder().node().build(x.clone());
            let y_var = x_var.broadcast(output_dim);
            assert_eq!(y.is_some(), y_var.is_some());
            if let Some((y, y_var)) = y.zip(y_var) {
                assert_eq!(y.shape(), y_var.shape());
                assert_eq!(y.strides(), y_var.value().strides());
                y_var.node().unwrap().backward().unwrap();
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[test]
fn tensor_dot_f32_m2_k2_n2_nn() {
    use linalg::Transpose;
    linalg::tensor_dot::<f32>(&Device::host(), [2, 2, 2], [Transpose::N, Transpose::N]);
}
