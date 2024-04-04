use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::os::windows::ffi::OsStringExt;
use std::{mem, ptr};
use windows::Win32::Foundation::{E_FAIL, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{GetVolumeNameForVolumeMountPointW, GetVolumePathNamesForVolumeNameW, WriteFile};
use windows::Win32::System::Ioctl::{
    PropertyStandardQuery, StorageDeviceProperty, GET_LENGTH_INFORMATION, IOCTL_DISK_GET_LENGTH_INFO, IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY
};

use windows::Win32::System::IO::DeviceIoControl;
use windows::{
    core::{Error, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE},
        Storage::FileSystem::{
            CreateFileW, FindFirstVolumeW, FindNextVolumeW,
            FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    },
};

#[derive(Debug)]
struct Win32DeviceInfo {
    path: String,
    serial_number: String,
    product_id: String,
    model: String,
    manufacturer: String,
    removable: bool,
    size: i64,
}

pub fn list_volumes() -> Result<(), Error> {
    unsafe {
        let mut volume_name = [0u16; 1024];

        // Buscar el primer volumen
        let mut result: HANDLE = FindFirstVolumeW(volume_name.as_mut())?;

        loop {
            let volume_name_str = String::from_utf16_lossy(&volume_name);
            println!("Volume: {}", volume_name_str.trim_end_matches('\0'));
            // Aquí puedes continuar con el manejo del handle del volumen

            let test = get_volume_info(volume_name.as_ptr());

            println!("Test: {:?}", test);

            // Buscar el siguiente volumen
            result = match FindNextVolumeW(result, volume_name.as_mut()) {
                Ok(_) => result,
                Err(error) => {
                    /*                 if let Error::HRESULT(hr) = error {
                        if hr.0 == windows::HRESULT_FROM_WIN32(winapi::shared::winerror::ERROR_NO_MORE_FILES) {
                            // No hay más volúmenes para encontrar
                            break;
                        }
                    } */
                    // Manejar otros errores
                    return Err(error);
                }
            };
        }
    }
}

pub fn write_to_device(
    handle: HANDLE,
    file_path: &str,
    block_size: usize,
    mut progress_callback: impl FnMut(usize, usize),
) -> windows::core::Result<()> {
    let mut file = File::open(file_path)?;

    // Determinar el tamaño total del archivo para el cálculo del progreso
    let total_size = file.metadata()?.len() as usize;
    let mut offset = 0;
    let mut buffer = vec![0u8; block_size];

    unsafe {
        while let Ok(bytes_read) = file.read(&mut buffer) {
            if bytes_read == 0 {
                break; // Final del archivo.
            }
            let current_slice = &buffer[..bytes_read];

            let mut bytes_written: u32 = 0;
            WriteFile(handle, Some(current_slice), Some(&mut bytes_written), None)?;

            if bytes_written as usize != bytes_read {
                // Si no se escribieron todos los bytes leídos, maneja este caso como un error.
                return Err(windows::core::Error::new(
                    E_FAIL,
                    "Failed to write all bytes to device.",
                ));
            }

            offset += bytes_written as usize;

            // Llamada al callback de progreso con la cantidad de bytes escritos y el tamaño total.
            progress_callback(offset, total_size);
        }
    }

    Ok(())
}

pub fn open_device(device_path: &str) -> windows::core::Result<HANDLE> {
    let device_path_w: Vec<u16> = device_path
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let device_path_pwstr: PCWSTR = PCWSTR(device_path_w.as_ptr());
    let access_rights: u32 = (GENERIC_READ | GENERIC_WRITE).0;

    let handle = unsafe {
        CreateFileW(
            device_path_pwstr,
            access_rights,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_DELETE_ON_CLOSE,
            None,
        )
    };

    if handle == Ok(INVALID_HANDLE_VALUE) {
        // Devuelve un error si el handle es inválido
        Err(windows::core::Error::from_win32())
    } else {
        // Si el handle es válido, devuélvelo
        handle
    }
}

pub fn list_physical_disks() {
    let mut drive_number = 0;
    let mut drives: Vec<Win32DeviceInfo> = Vec::new();

    loop {
        let drive_name = format!("\\\\.\\PhysicalDrive{}", drive_number);
        let device_path_w: Vec<u16> = drive_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let device_path_pwstr: PCWSTR = PCWSTR(device_path_w.as_ptr());

        let access_rights: u32 = (GENERIC_READ | GENERIC_WRITE).0;

        let handle_result = unsafe {
            CreateFileW(
                device_path_pwstr,
                access_rights,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_DELETE_ON_CLOSE,
                None,
            )
        };

        if let Ok(handle) = handle_result {

            let result = get_device_info(handle);
            let size = get_device_size(handle);

            match result {
                Ok(mut info) => {
                    info.path = drive_name;
                    info.size = size;
                    drives.push(info);
                }
                Err(err) => println!("{:?}", err)
            }

            unsafe {
                let _ = CloseHandle(handle);
            }
        } else {
            break;
        }
        drive_number += 1;
    }

    for (index, drive) in drives.iter().enumerate() {
      println!("Drive {}:\n  Path: {}\n  Product ID: {}\n  Size: {} GB\n  Removable: {}",
          index + 1,
          drive.path,
          drive.product_id,
          drive.size / 1_024_000_000, // Convertir bytes a GB para mejor legibilidad
          if drive.removable { "Yes" } else { "No" }
      );
  }
}

fn get_device_info(handle: HANDLE) -> Result<Win32DeviceInfo, Error> {
    let mut query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };

    let mut property_buffer: [u8; 1024] = [0; 1024];
    let mut returned_bytes: u32 = 0;

    let result = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            Some(&mut query as *mut _ as *mut _),
            mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            Some(property_buffer.as_mut_ptr() as *mut _),
            property_buffer.len() as u32,
            Some(&mut returned_bytes),
            Some(ptr::null_mut()),
        )
    };

    // Función auxiliar para obtener strings del descriptor
    let get_string_from_descriptor = |offset: u32| -> String {
        if offset > 0 {
            unsafe {
                let c_str_ptr = property_buffer.as_ptr().add(offset as usize) as *const i8;
                std::ffi::CStr::from_ptr(c_str_ptr)
                    .to_string_lossy()
                    .into_owned()
            }
        } else {
            String::from("")
        }
    };

    match result {
        Ok(()) => {
            let descriptor: &STORAGE_DEVICE_DESCRIPTOR = unsafe { &*(property_buffer.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
            let serial_number: String = get_string_from_descriptor(descriptor.SerialNumberOffset);
            let product_id: String = get_string_from_descriptor(descriptor.ProductIdOffset);
            let model: String = get_string_from_descriptor(descriptor.ProductRevisionOffset);
            let manufacturer: String = get_string_from_descriptor(descriptor.VendorIdOffset);

            Ok(Win32DeviceInfo {
                serial_number,
                product_id,
                model,
                manufacturer,
                path: String::from(""),
                removable: descriptor.RemovableMedia.into(),
                size: 0
            })
        }
        Err(error) => {
            println!("Error: {:?}", error);
            Err(windows::core::Error::from_win32())
        }
    }
}

fn get_device_size(handle: HANDLE) -> i64 {

    let mut property_buffer: [u8; 1024] = [0; 1024];
    let mut returned_bytes: u32 = 0;
    
    let result = unsafe {

        DeviceIoControl(
            handle,
            IOCTL_DISK_GET_LENGTH_INFO,
            Some(ptr::null_mut()),
            0,
            Some(property_buffer.as_mut_ptr() as *mut _),
            property_buffer.len() as u32,
            Some(&mut returned_bytes),
            Some(ptr::null_mut()),
        )
    };

    match result {
        Ok(()) => {
            let descriptor: &GET_LENGTH_INFORMATION = unsafe { &*(property_buffer.as_ptr() as *const GET_LENGTH_INFORMATION) };
            let size = descriptor.Length;
            size
        }
        Err(err) => {
            println!("{:?}", err);
            0
        }
    }
}

fn get_volumes_for_disk(disk_path: &str) {
  
  const MAX_BUFFER_SIZE: usize = 4096;

  let device_path_w: Vec<u16> = disk_path
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
  let volume_mount_point: PCWSTR =  PCWSTR(device_path_w.as_ptr());
  let mut volume_name_buffer = [0u16; MAX_BUFFER_SIZE];
  let mut return_length: u32 = 0;

  /* let result = unsafe {
    GetVolumeNameForVolumeMountPointW(
        disk_path.encode_utf16().chain(Some(0)).collect::<Vec<_>>().as_ptr(),
        volume_name_buffer.as_mut_ptr(),
    )
  }; */

}

fn get_volume_info(volume_name: *const u16) {
  let volume_path_pwstr: PCWSTR = PCWSTR(volume_name);
  let mut path_names_buffer: Vec<u16> = vec![0; 1024];
  let mut return_length: u32 = 0;

  let result: Result<(), Error> = unsafe { GetVolumePathNamesForVolumeNameW(
      volume_path_pwstr,
      Some(&mut path_names_buffer),
      &mut return_length
    )
  };

  match result {
    Ok(()) => {
      let path_names = OsString::from_wide(&path_names_buffer[..(return_length as usize)]);
      println!("Volume path: {:?}", path_names);
    }
    Err(err) => {
        println!("{:?}", err);
    }
  }

}