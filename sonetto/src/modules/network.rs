use std::ffi::CString;

use super::{MhyContext, MhyModule, ModuleType};
use crate::util::{il2cpp_string_new, read_csharp_string};
use anyhow::Result;
use ilhook::x64::Registers;

const WEB_REQUEST_UTILS_MAKE_INITIAL_URL: usize = 0x3E7D6A0;
const BROWSER_LOAD_URL: usize = 0x3EA9630;
const SET_REQUEST_HEADER: usize = 0x3E78660;

pub struct Network;

impl MhyModule for MhyContext<Network> {
    unsafe fn init(&mut self) -> Result<()> {
        crate::config::get().map_err(anyhow::Error::msg)?;
        self.interceptor.attach(
            self.assembly_base + WEB_REQUEST_UTILS_MAKE_INITIAL_URL,
            on_make_initial_url,
        )?;

        self.interceptor
            .attach(self.assembly_base + BROWSER_LOAD_URL, on_browser_load_url)?;

        self.interceptor.attach(
            self.assembly_base + SET_REQUEST_HEADER,
            on_set_request_header,
        )?;

        Ok(())
    }

    unsafe fn de_init(&mut self) -> Result<()> {
        Ok(())
    }

    fn get_module_type(&self) -> super::ModuleType {
        ModuleType::Network
    }
}

unsafe extern "win64" fn on_make_initial_url(reg: *mut Registers, _: usize) {
    let url = read_csharp_string((*reg).rcx as usize);
    let Ok(config) = crate::config::get() else {
        return;
    };
    if let Some(new_url) = rewrite_url(&url, "http", &config.sdk.host, config.sdk.port) {
        println!("Redirect: {url} -> {new_url}");
        let cstr = CString::new(new_url.as_str()).unwrap();
        let new_ptr = il2cpp_string_new(cstr.as_ptr() as *const u8);
        if new_ptr == 0 {
            println!("[ERROR] il2cpp_string_new export was unavailable");
            return;
        }
        (*reg).rcx = new_ptr as u64;
    }
}

unsafe extern "win64" fn on_browser_load_url(reg: *mut Registers, _: usize) {
    let url_ptr = (*reg).rdx as usize;
    if url_ptr == 0 {
        return;
    }

    let url = read_csharp_string(url_ptr);

    if url == "about:blank" {
        return;
    }
    let Ok(config) = crate::config::get() else {
        return;
    };
    let Some(new_url) = rewrite_url(&url, "https", &config.tls.host, config.tls.port) else {
        return;
    };
    println!("Browser::LoadURL: {url} -> {new_url}");
    let cstr = CString::new(new_url).unwrap();
    let new_ptr = il2cpp_string_new(cstr.as_ptr() as *const u8);
    if new_ptr != 0 {
        (*reg).rdx = new_ptr as u64;
    }
}

unsafe extern "win64" fn on_set_request_header(reg: *mut Registers, _: usize) {
    if (*reg).rdx == 0 || (*reg).r8 == 0 {
        return;
    }
    let key = read_csharp_string((*reg).rdx as usize);
    let value = read_csharp_string((*reg).r8 as usize);

    if key.is_empty() || value.is_empty() {
        return;
    }

    if key.eq_ignore_ascii_case("host") {
        let Ok(config) = crate::config::get() else {
            return;
        };
        let host_value = format!("{}:{}", config.sdk.host, config.sdk.port);
        println!("[SetRequestHeader] Rewriting Host: {value} -> {host_value}");
        let Ok(host) = CString::new(host_value) else {
            return;
        };
        let new_ptr = il2cpp_string_new(host.as_ptr() as *const u8);
        if new_ptr != 0 {
            (*reg).r8 = new_ptr as u64;
        }
    } else {
        println!("[SetRequestHeader] {key}: {value}");
    }
}

fn rewrite_url(url: &str, scheme: &str, host: &str, port: u16) -> Option<String> {
    if !(url.contains("sl916.com") || url.contains("game.local")) || url.contains("C:/") {
        return None;
    }
    let (_, rest) = url.split_once("://")?;
    let path = rest.find('/').map(|index| &rest[index..]).unwrap_or("/");
    Some(format!("{scheme}://{host}:{port}{path}"))
}

#[cfg(test)]
mod tests {
    use super::rewrite_url;

    #[test]
    fn sdk_urls_keep_path_and_use_public_sdk_endpoint() {
        assert_eq!(
            rewrite_url(
                "https://api.sl916.com/login/config?game=60001",
                "http",
                "reverse1999.yezimoan.xyz",
                32051,
            ),
            Some("http://reverse1999.yezimoan.xyz:32051/login/config?game=60001".to_string())
        );
    }

    #[test]
    fn local_files_and_unrelated_urls_are_not_rewritten() {
        assert_eq!(
            rewrite_url("file:///C:/game/config.json", "http", "example.com", 80),
            None
        );
        assert_eq!(
            rewrite_url("https://example.com/keep", "http", "example.com", 80),
            None
        );
    }
}
