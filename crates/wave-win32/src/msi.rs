//! Reads of the MSI product database.

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::ApplicationInstallationAndServicing::{
    MsiEnumProductsW, MsiGetProductInfoW, INSTALLPROPERTY_ASSIGNMENTTYPE,
    INSTALLPROPERTY_INSTALLDATE, INSTALLPROPERTY_INSTALLLOCATION, INSTALLPROPERTY_PRODUCTNAME,
    INSTALLPROPERTY_PUBLISHER, INSTALLPROPERTY_VERSIONSTRING,
};

use wave_core::error::Result;
use wave_core::provider::MsiProduct;

/// A product code is always `{8-4-4-4-12}` plus a terminator.
const PRODUCT_CODE_LEN: usize = 39;

/// Every product in the database.
pub fn products() -> Result<Vec<MsiProduct>> {
    let mut products = Vec::new();
    let mut index = 0u32;

    loop {
        let mut buffer = [0u16; PRODUCT_CODE_LEN];
        let status = unsafe { MsiEnumProductsW(index, PWSTR(buffer.as_mut_ptr())) };

        if status == ERROR_NO_MORE_ITEMS.0 {
            break;
        }
        if status != ERROR_SUCCESS.0 {
            // Stop rather than abort: a partial product list still lets the
            // inventory merge what it has, and wave-core surfaces the gap.
            tracing::warn!(index, status, "MsiEnumProductsW failed; stopping enumeration");
            break;
        }

        let code = trim_nul(&buffer);
        if !code.is_empty() {
            products.push(read_product(&code));
        }
        index += 1;
    }

    Ok(products)
}

fn read_product(product_code: &str) -> MsiProduct {
    let product = HSTRING::from(product_code);
    let property = |attribute: PCWSTR| info(&product, attribute);

    MsiProduct {
        name: property(INSTALLPROPERTY_PRODUCTNAME),
        version: property(INSTALLPROPERTY_VERSIONSTRING),
        publisher: property(INSTALLPROPERTY_PUBLISHER),
        install_location: property(INSTALLPROPERTY_INSTALLLOCATION),
        install_date: property(INSTALLPROPERTY_INSTALLDATE),
        // AssignmentType 1 means the product is installed for every user.
        machine_scoped: property(INSTALLPROPERTY_ASSIGNMENTTYPE).as_deref() == Some("1"),
        product_code: product_code.to_string(),
    }
}

/// One product property, or `None` when it is absent or unreadable.
fn info(product: &HSTRING, attribute: PCWSTR) -> Option<String> {
    let mut length = 0u32;

    // With no buffer, MSI reports the length in characters, excluding the
    // terminator.
    let status = unsafe { MsiGetProductInfoW(product, attribute, None, Some(&mut length)) };
    if status != ERROR_SUCCESS.0 && status != ERROR_MORE_DATA.0 {
        return None;
    }
    if length == 0 {
        return None;
    }

    let mut buffer = vec![0u16; length as usize + 1];
    let mut capacity = buffer.len() as u32;
    let status = unsafe {
        MsiGetProductInfoW(
            product,
            attribute,
            Some(PWSTR(buffer.as_mut_ptr())),
            Some(&mut capacity),
        )
    };
    if status != ERROR_SUCCESS.0 {
        return None;
    }

    let value = trim_nul(&buffer);
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

fn trim_nul(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}
