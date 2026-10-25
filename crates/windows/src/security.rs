use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::SystemServices::{SE_GROUP_ENABLED, SE_GROUP_USE_FOR_DENY_ONLY};
use windows_sys::Win32::{
    Foundation::*,
    Security::Authorization::*,
    Security::*,
    Storage::FileSystem::*,
    System::{Pipes::*, Threading::*},
};

fn wide(value: &std::ffi::OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL en ruta Windows",
        ));
    }
    value.push(0);
    Ok(value)
}
fn last() -> io::Error {
    io::Error::last_os_error()
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn descriptor(sddl: &str) -> io::Result<Local> {
    let text = wide(std::ffi::OsStr::new(sddl))?;
    let mut sd = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), 1, &mut sd, null_mut())
    } == 0
    {
        return Err(last());
    }
    Ok(Local(sd))
}
fn process_token(process: HANDLE) -> io::Result<Handle> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(last());
    }
    Ok(Handle(token))
}
fn user_sid(token: HANDLE) -> io::Result<String> {
    let mut len = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
    }
    if len == 0 {
        return Err(last());
    }
    // Vec<usize> conserva la alineación exigida por TOKEN_USER.
    let mut data = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token, TokenUser, data.as_mut_ptr().cast(), len, &mut len) }
        == 0
    {
        return Err(last());
    }
    let user = unsafe { &*data.as_ptr().cast::<TOKEN_USER>() };
    sid_string(user.User.Sid)
}
fn sid_string(sid: PSID) -> io::Result<String> {
    let mut text = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(last());
    }
    let allocation = Local(text.cast());
    let mut len = 0;
    unsafe {
        while *text.add(len) != 0 {
            len += 1;
        }
    }
    let result = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    drop(allocation);
    Ok(result)
}
fn privileged(token: HANDLE) -> io::Result<bool> {
    if user_sid(token)? == "S-1-5-18" {
        return Ok(true);
    }
    let mut len = 0;
    unsafe {
        GetTokenInformation(token, TokenGroups, null_mut(), 0, &mut len);
    }
    if len == 0 {
        return Err(last());
    }
    let mut data = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token, TokenGroups, data.as_mut_ptr().cast(), len, &mut len) }
        == 0
    {
        return Err(last());
    }
    let groups = unsafe { &*data.as_ptr().cast::<TOKEN_GROUPS>() };
    let entries =
        unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize) };
    for group in entries {
        if group.Attributes & SE_GROUP_ENABLED as u32 != 0
            && group.Attributes & SE_GROUP_USE_FOR_DENY_ONLY as u32 == 0
            && sid_string(group.Sid)? == "S-1-5-32-544"
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Crea exclusivamente con DACL protegida, sin ventana de permisos heredados.
pub fn create_private(path: &Path) -> io::Result<File> {
    if !privileged(process_token(unsafe { GetCurrentProcess() })?.0)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "la custodia de claves exige SYSTEM o Administrator elevado",
        ));
    }
    let sd = descriptor("O:BAG:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)")?;
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let path = wide(path.as_os_str())?;
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_WRITE | READ_CONTROL,
            0,
            &attrs,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(last());
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}
/// Valida el mismo objeto abierto que se leerá; rechaza enlaces y ACL públicas.
pub fn read_private(path: &Path) -> io::Result<Vec<u8>> {
    let path = wide(path.as_os_str())?;
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_READ | READ_CONTROL,
            FILE_SHARE_READ,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(last());
    }
    let mut file = unsafe { File::from_raw_handle(handle) };
    let mut info = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return Err(last());
    }
    if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "se exige fichero privado regular",
        ));
    }
    validate_dacl(&file)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}
fn validate_dacl(file: &File) -> io::Result<()> {
    let mut acl = null_mut();
    let mut sd = null_mut();
    let mut owner = null_mut();
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut sd,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let _sd = Local(sd);
    let reject = || {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "DACL privada requerida: sólo SYSTEM y Administrators",
        )
    };
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) } == 0 {
        return Err(last());
    }
    if control & SE_DACL_PROTECTED == 0 {
        return Err(reject());
    }
    if acl.is_null() {
        return Err(reject());
    }
    if owner.is_null() {
        return Err(reject());
    }
    let owner = sid_string(owner)?;
    if owner != "S-1-5-18" && owner != "S-1-5-32-544" {
        return Err(reject());
    }
    for index in 0..unsafe { (*acl).AceCount } {
        let mut ace = null_mut();
        if unsafe { GetAce(acl, u32::from(index), &mut ace) } == 0 {
            return Err(last());
        }
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        if header.AceType == 1 {
            continue;
        } // ACCESS_DENIED_ACE_TYPE
        if header.AceType != 0 {
            return Err(reject());
        } // ACCESS_ALLOWED_ACE_TYPE
        let allow = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = sid_string(std::ptr::addr_of!(allow.SidStart).cast_mut().cast())?;
        if sid != "S-1-5-18" && sid != "S-1-5-32-544" {
            return Err(reject());
        }
    }
    Ok(())
}
pub fn replace_private(path: &Path, body: &[u8]) -> io::Result<()> {
    // El temporal exclusivo vive en el mismo volumen y nunca reutiliza un objeto ajeno.
    let mut attempt = 0u32;
    let (tmp, mut file) = loop {
        let candidate =
            path.with_file_name(format!(".atenea-{}-{attempt}.tmp", std::process::id()));
        match create_private(&candidate) {
            Ok(file) => break (candidate, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && attempt < 128 => attempt += 1,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        file.write_all(body)?;
        file.sync_all()?;
        drop(file);
        let from = wide(tmp.as_os_str())?;
        let to = wide(path.as_os_str())?;
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(last());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

pub fn validate_pipe_path(path: &Path) -> io::Result<()> {
    let text = path
        .to_str()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "nombre de pipe inválido"))?;
    let prefix = r"\\.\pipe\";
    if !text.starts_with(prefix)
        || text.len() == prefix.len()
        || text[prefix.len()..].contains(['\\', '/', '\0'])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "se exige \\\\.\\pipe\\<nombre> local",
        ));
    }
    Ok(())
}
pub fn create_pipe(
    path: &Path,
    group: Option<u32>,
    first: bool,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    validate_pipe_path(path)?;
    let extra = match group {
        None | Some(544) => String::new(),
        // No FILE_CREATE_PIPE_INSTANCE: un lector no puede suplantar al servidor.
        Some(545) => "(A;;0x120003;;;BU)".into(),
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "status_gid Windows admite 544 (Administrators) o 545 (Users)",
            ));
        }
    };
    let sd = descriptor(&format!("O:BAG:BAD:P(A;;GA;;;SY)(A;;GA;;;BA){extra}"))?;
    let mut attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    unsafe {
        tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                path,
                (&mut attrs as *mut SECURITY_ATTRIBUTES).cast(),
            )
    }
}
/// Impersona sólo durante la consulta síncrona; siempre revierte antes de volver a Tokio.
pub fn authorize_pipe_client(pipe: &impl AsRawHandle) -> io::Result<bool> {
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            // Continuar impersonado contaminaría otras tareas del mismo hilo.
            if unsafe { RevertToSelf() } == 0 {
                std::process::abort();
            }
        }
    }
    if unsafe { ImpersonateNamedPipeClient(pipe.as_raw_handle()) } == 0 {
        return Err(last());
    }
    let _revert = Revert;
    let mut token = null_mut();
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
        return Err(last());
    }
    let token = Handle(token);
    privileged(token.0)
}
pub fn connect_pipe(path: &Path) -> io::Result<File> {
    validate_pipe_path(path)?;
    let path = wide(path.as_os_str())?;
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_READ_DATA | FILE_WRITE_DATA | SYNCHRONIZE | READ_CONTROL,
            0,
            null(),
            OPEN_EXISTING,
            SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(last());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    // Leer el token de LocalSystem exigiría SeDebugPrivilege al lector de estado.
    // El propietario BA fijado al crear el pipe no lo puede suplantar un usuario estándar.
    let mut owner = null_mut();
    let mut sd = null_mut();
    let code = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut sd,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let _sd = Local(sd);
    if owner.is_null() || sid_string(owner)? != "S-1-5-32-544" {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "propietario del servidor IPC no es Administrators",
        ));
    }
    Ok(file)
}

/// Espera acotada sin hilo bloqueado: PeekNamedPipe limita cada lectura disponible.
pub fn pipe_reply(mut file: File, timeout: std::time::Duration) -> io::Result<String> {
    let deadline = std::time::Instant::now() + timeout;
    let mut reply = Vec::new();
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "tiempo de espera IPC agotado",
            ));
        }
        let mut available = 0;
        if unsafe {
            PeekNamedPipe(
                file.as_raw_handle(),
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        } == 0
        {
            return Err(last());
        }
        if available > 0 {
            let mut buffer = [0u8; 4096];
            let count = file.read(&mut buffer[..(available as usize).min(4096)])?;
            if count == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "pipe cerrada"));
            }
            reply.extend_from_slice(&buffer[..count]);
            if reply.len() > 65536 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "respuesta IPC demasiado grande",
                ));
            }
            if let Some(end) = reply.iter().position(|b| *b == b'\n') {
                return String::from_utf8(reply[..end].to_vec()).map_err(io::Error::other);
            }
        } else if std::time::Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "tiempo de espera IPC agotado",
            ));
        } else {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_remote_and_nested_names() {
        assert!(validate_pipe_path(Path::new(r"\\.\pipe\atenea-control")).is_ok());
        for path in [r"\\remote\pipe\atenea", r"\\.\pipe\", r"\\.\pipe\a\b"] {
            assert!(validate_pipe_path(Path::new(path)).is_err());
        }
    }
    #[test]
    fn private_files_are_exclusive_and_replaceable() {
        let dir = std::env::temp_dir().join(format!("atenea-dacl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key");
        if let Err(error) = create_private(&path) {
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert!(!privileged(process_token(unsafe { GetCurrentProcess() }).unwrap().0).unwrap());
            std::fs::remove_dir_all(dir).unwrap();
            return;
        }
        assert_eq!(
            create_private(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        replace_private(&path, b"secret").unwrap();
        assert_eq!(read_private(&path).unwrap(), b"secret");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rejects_public_secret_dacl() {
        let path = std::env::temp_dir().join(format!("atenea-public-{}.key", std::process::id()));
        let sd = descriptor("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GA;;;BU)").unwrap();
        let attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let name = wide(path.as_os_str()).unwrap();
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_WRITE,
                0,
                &attrs,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);
        drop(unsafe { File::from_raw_handle(handle) });
        assert_eq!(
            read_private(&path).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        std::fs::remove_file(path).unwrap();
    }
}
