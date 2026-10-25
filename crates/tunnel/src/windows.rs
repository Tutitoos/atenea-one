//! Wintun se carga por ruta absoluta; sólo se usa el controlador, nunca WireGuard.
#![allow(unsafe_code)]
use libloading::Library;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_NO_MORE_ITEMS, HANDLE, WAIT_FAILED, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects,
};

type Create =
    unsafe extern "system" fn(*const u16, *const u16, *const windows_sys::core::GUID) -> HANDLE;
type Close = unsafe extern "system" fn(HANDLE);
type Start = unsafe extern "system" fn(HANDLE, u32) -> HANDLE;
type Event = unsafe extern "system" fn(HANDLE) -> HANDLE;
type Receive = unsafe extern "system" fn(HANDLE, *mut u32) -> *mut u8;
type Release = unsafe extern "system" fn(HANDLE, *const u8);
type Allocate = unsafe extern "system" fn(HANDLE, u32) -> *mut u8;

struct Session {
    // La DLL debe sobrevivir a todos los punteros de función y handles.
    _library: Library,
    adapter: HANDLE,
    session: HANDLE,
    stop: HANDLE,
    close: Close,
    end: Close,
    receive: Receive,
    release: Release,
    allocate: Allocate,
    send: Release,
    read_event: Event,
    wait_error: AtomicU32,
}
// SAFETY: Wintun documenta estas operaciones como thread-safe; el cierre ocurre tras join.
unsafe impl Send for Session {}
unsafe impl Sync for Session {}
impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: handles propios, creados en orden y cerrados una sola vez.
        unsafe {
            (self.end)(self.session);
            (self.close)(self.adapter);
            CloseHandle(self.stop);
        }
    }
}

pub struct Tun {
    name: String,
    inner: Arc<Session>,
    request: Option<mpsc::SyncSender<()>>,
    ready: Arc<tokio::sync::Notify>,
    worker: Option<std::thread::JoinHandle<()>>,
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "paquete Wintun/IP inválido")
}
fn validate(packet: &[u8]) -> io::Result<()> {
    match packet.first().map(|byte| byte >> 4) {
        Some(4)
            if packet.len() >= 20
                && usize::from(packet[0] & 15) * 4 >= 20
                && usize::from(packet[0] & 15) * 4 <= packet.len()
                && usize::from(u16::from_be_bytes([packet[2], packet[3]])) == packet.len() =>
        {
            Ok(())
        }
        Some(6)
            if packet.len() >= 40
                && usize::from(u16::from_be_bytes([packet[4], packet[5]])) + 40 == packet.len()
                && packet.len() <= u16::MAX as usize =>
        {
            Ok(())
        }
        _ => Err(invalid()),
    }
}
fn dll_path() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("ATENEA_WINTUN_DLL") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "ATENEA_WINTUN_DLL debe ser absoluta",
            ));
        }
        return Ok(path);
    }
    let executable = std::env::current_exe()?;
    Ok(executable
        .parent()
        .ok_or_else(|| io::Error::other("ejecutable sin directorio"))?
        .join("wintun.dll"))
}
fn wide(text: &str) -> io::Result<Vec<u16>> {
    if text.is_empty() || text.contains('\0') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "nombre Wintun inválido",
        ));
    }
    Ok(text.encode_utf16().chain(Some(0)).collect())
}
impl Tun {
    pub fn create(name: &str) -> io::Result<Self> {
        let adapter_name = wide(name)?;
        let tunnel_type = wide("Atenea")?;
        let path = dll_path()?;
        // SAFETY: ruta absoluta de instalación privada; ABI de Wintun 0.14.1 y símbolos comprobados.
        let inner = unsafe {
            let library = Library::new(&path).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "Wintun: no se puede cargar {} (instale la DLL oficial verificada): {e}",
                        path.display()
                    ),
                )
            })?;
            macro_rules! symbol {
                ($name:literal, $ty:ty) => {
                    *library
                        .get::<$ty>(concat!($name, "\0").as_bytes())
                        .map_err(io::Error::other)?
                };
            }
            let create = symbol!("WintunCreateAdapter", Create);
            let close = symbol!("WintunCloseAdapter", Close);
            let start = symbol!("WintunStartSession", Start);
            let end = symbol!("WintunEndSession", Close);
            let read_event = symbol!("WintunGetReadWaitEvent", Event);
            let receive = symbol!("WintunReceivePacket", Receive);
            let release = symbol!("WintunReleaseReceivePacket", Release);
            let allocate = symbol!("WintunAllocateSendPacket", Allocate);
            let send = symbol!("WintunSendPacket", Release);
            let stop = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
            if stop.is_null() {
                return Err(io::Error::last_os_error());
            }
            let adapter = create(
                adapter_name.as_ptr(),
                tunnel_type.as_ptr(),
                std::ptr::null(),
            );
            if adapter.is_null() {
                let error = io::Error::last_os_error();
                CloseHandle(stop);
                return Err(error);
            }
            let session = start(adapter, 0x400000);
            if session.is_null() {
                let error = io::Error::last_os_error();
                close(adapter);
                CloseHandle(stop);
                return Err(error);
            }
            Arc::new(Session {
                _library: library,
                adapter,
                session,
                stop,
                close,
                end,
                receive,
                release,
                allocate,
                send,
                read_event,
                wait_error: AtomicU32::new(0),
            })
        };
        let (request, requests) = mpsc::sync_channel(1);
        let ready = Arc::new(tokio::sync::Notify::new());
        let (session, notify) = (inner.clone(), ready.clone());
        let worker = std::thread::Builder::new()
            .name("atenea-wintun-read".into())
            .spawn(move || {
                while requests.recv().is_ok() {
                    // SAFETY: Arc mantiene la sesión y ambos eventos vivos hasta terminar el hilo.
                    let result = unsafe {
                        let events = [session.stop, (session.read_event)(session.session)];
                        WaitForMultipleObjects(2, events.as_ptr(), 0, INFINITE)
                    };
                    if result == WAIT_OBJECT_0 {
                        break;
                    }
                    if result == WAIT_FAILED {
                        session.wait_error.store(
                            io::Error::last_os_error().raw_os_error().unwrap_or(1) as u32,
                            Ordering::Release,
                        );
                    }
                    notify.notify_one();
                    if result == WAIT_FAILED {
                        break;
                    }
                }
            })?;
        Ok(Self {
            name: name.into(),
            inner,
            request: Some(request),
            ready,
            worker: Some(worker),
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub async fn read_packet(&self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            let error = self.inner.wait_error.load(Ordering::Acquire);
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            // SAFETY: cada paquete recibido pertenece a esta sesión y se libera antes de retornar.
            unsafe {
                let mut length = 0;
                let packet = (self.inner.receive)(self.inner.session, &mut length);
                if !packet.is_null() {
                    let bytes = std::slice::from_raw_parts(packet, length as usize);
                    let result = if bytes.len() > buffer.len() {
                        Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "búfer IP insuficiente",
                        ))
                    } else {
                        validate(bytes).map(|()| {
                            buffer[..bytes.len()].copy_from_slice(bytes);
                            bytes.len()
                        })
                    };
                    (self.inner.release)(self.inner.session, packet);
                    return result;
                }
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_NO_MORE_ITEMS as i32) {
                    return Err(error);
                }
            }
            if let Some(sender) = &self.request {
                match sender.try_send(()) {
                    Ok(()) | Err(mpsc::TrySendError::Full(())) => {}
                    Err(mpsc::TrySendError::Disconnected(())) => {
                        return Err(io::Error::other("hilo Wintun detenido"));
                    }
                }
            }
            self.ready.notified().await;
        }
    }
    pub async fn write_packet(&self, packet: &[u8]) -> io::Result<usize> {
        validate(packet)?;
        // SAFETY: la reserva es del tamaño exacto y SendPacket transfiere su propiedad a Wintun.
        unsafe {
            let output = (self.inner.allocate)(self.inner.session, packet.len() as u32);
            if output.is_null() {
                return Err(io::Error::last_os_error());
            }
            std::ptr::copy_nonoverlapping(packet.as_ptr(), output, packet.len());
            (self.inner.send)(self.inner.session, output);
        }
        Ok(packet.len())
    }
}
impl Drop for Tun {
    fn drop(&mut self) {
        // SAFETY: stop permanece vivo hasta después de join y del último Arc.
        unsafe {
            SetEvent(self.inner.stop);
        }
        self.request.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_ip_validation_rejects_headers_truncation_and_oversize() {
        let mut v4 = [0; 20];
        v4[0] = 0x45;
        v4[3] = 20;
        assert!(validate(&v4).is_ok());
        assert!(validate(&v4[..19]).is_err());
        v4[0] = 0x44;
        assert!(validate(&v4).is_err());
        let mut v6 = [0; 40];
        v6[0] = 0x60;
        assert!(validate(&v6).is_ok());
        v6[5] = 1;
        assert!(validate(&v6).is_err());
        assert!(validate(&[]).is_err());
        // El agente descarta estos paquetes por su tipo: solo `InvalidData`.
        assert_eq!(
            validate(&[]).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(wide("a\0b").is_err());
    }
}
