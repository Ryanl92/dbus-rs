use std::borrow::Cow;
use std::{fmt, mem, ptr};
use super::{ffi, Error, MessageType, TypeSig, libc, to_c_str, c_str_to_slice, init_dbus};
use std::os::unix::io::{RawFd, AsRawFd};

fn new_dbus_message_iter() -> ffi::DBusMessageIter {
    ffi::DBusMessageIter {
        dummy1: ptr::null_mut(),
        dummy2: ptr::null_mut(),
        dummy3: 0,
        dummy4: 0,
        dummy5: 0,
        dummy6: 0,
        dummy7: 0,
        dummy8: 0,
        dummy9: 0,
        dummy10: 0,
        dummy11: 0,
        pad1: 0,
        pad2: 0,
        pad3: ptr::null_mut(),
    }
}

/// An RAII wrapper around Fd to ensure that file descriptor is closed
/// when the scope ends.
#[derive(Debug, PartialEq, PartialOrd)]
pub struct OwnedFd {
    fd: RawFd
}

impl OwnedFd {
    pub fn new(fd: RawFd) -> OwnedFd {
        OwnedFd { fd: fd }
    }

    pub fn into_fd(self) -> RawFd {
        let s = self.fd;
        unsafe { ::std::mem::forget(self); }
        s
    }
}

impl Drop for OwnedFd {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd); }
    }
}

impl Clone for OwnedFd {
    fn clone(&self) -> OwnedFd {
        OwnedFd::new(unsafe { libc::dup(self.fd) } ) // FIXME: handle errors
    }
}

impl AsRawFd for OwnedFd {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

/// MessageItem - used as parameters and return values from
/// method calls, or as data added to a signal.
#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub enum MessageItem {
    Array(Vec<MessageItem>, TypeSig<'static>),
    Struct(Vec<MessageItem>),
    Variant(Box<MessageItem>),
    DictEntry(Box<MessageItem>, Box<MessageItem>),
    ObjectPath(String),
    Str(String),
    Bool(bool),
    Byte(u8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    UInt16(u16),
    UInt32(u32),
    UInt64(u64),
    Double(f64),
    UnixFd(OwnedFd),
}

fn iter_get_basic(i: &mut ffi::DBusMessageIter) -> i64 {
    let mut c: i64 = 0;
    unsafe {
        let p: *mut libc::c_void = mem::transmute(&mut c);
        ffi::dbus_message_iter_get_basic(i, p);
    }
    c
}

fn iter_get_f64(i: &mut ffi::DBusMessageIter) -> f64 {
    let mut c: f64 = 0.0;
    unsafe {
        let p: *mut libc::c_void = mem::transmute(&mut c);
        ffi::dbus_message_iter_get_basic(i, p);
    }
    c
}

fn iter_append_f64(i: &mut ffi::DBusMessageIter, v: f64) {
    unsafe {
        let p: *const libc::c_void = mem::transmute(&v);
        ffi::dbus_message_iter_append_basic(i, ffi::DBUS_TYPE_DOUBLE, p);
    }
}

fn iter_append_array(i: &mut ffi::DBusMessageIter, a: &[MessageItem], t: TypeSig<'static>) {
    let mut subiter = new_dbus_message_iter();
    let atype = to_c_str(&t);

    assert!(unsafe { ffi::dbus_message_iter_open_container(i, ffi::DBUS_TYPE_ARRAY, atype.as_ptr(), &mut subiter) } != 0);
    for item in a.iter() {
//        assert!(item.type_sig() == t);
        item.iter_append(&mut subiter);
    }
    assert!(unsafe { ffi::dbus_message_iter_close_container(i, &mut subiter) } != 0);
}

fn iter_append_struct(i: &mut ffi::DBusMessageIter, a: &[MessageItem]) {
    let mut subiter = new_dbus_message_iter();
    let res = unsafe { ffi::dbus_message_iter_open_container(i, ffi::DBUS_TYPE_STRUCT, ptr::null(), &mut subiter) };
    assert!(res != 0);
    for item in a.iter() {
        item.iter_append(&mut subiter);
    }
    let res2 = unsafe { ffi::dbus_message_iter_close_container(i, &mut subiter) };
    assert!(res2 != 0);
}

fn iter_append_variant(i: &mut ffi::DBusMessageIter, a: &MessageItem) {
    let mut subiter = new_dbus_message_iter();
    let atype = to_c_str(&format!("{}", a.array_type() as u8 as char));
    assert!(unsafe { ffi::dbus_message_iter_open_container(i, ffi::DBUS_TYPE_VARIANT, atype.as_ptr(), &mut subiter) } != 0);
    a.iter_append(&mut subiter);
    assert!(unsafe { ffi::dbus_message_iter_close_container(i, &mut subiter) } != 0);
}

fn iter_append_dict(i: &mut ffi::DBusMessageIter, k: &MessageItem, v: &MessageItem) {
    let mut subiter = new_dbus_message_iter();
    assert!(unsafe { ffi::dbus_message_iter_open_container(i, ffi::DBUS_TYPE_DICT_ENTRY, ptr::null(), &mut subiter) } != 0);
    k.iter_append(&mut subiter);
    v.iter_append(&mut subiter);
    assert!(unsafe { ffi::dbus_message_iter_close_container(i, &mut subiter) } != 0);
}

impl MessageItem {

    pub fn type_sig(&self) -> TypeSig<'static> {
        match self {
            // TODO: Can we make use of the ffi constants here instead of duplicating them?
            &MessageItem::Str(_) => Cow::Borrowed("s"),
            &MessageItem::Bool(_) => Cow::Borrowed("b"),
            &MessageItem::Byte(_) => Cow::Borrowed("y"),
            &MessageItem::Int16(_) => Cow::Borrowed("n"),
            &MessageItem::Int32(_) => Cow::Borrowed("i"),
            &MessageItem::Int64(_) => Cow::Borrowed("x"),
            &MessageItem::UInt16(_) => Cow::Borrowed("q"),
            &MessageItem::UInt32(_) => Cow::Borrowed("u"),
            &MessageItem::UInt64(_) => Cow::Borrowed("t"),
            &MessageItem::Double(_) => Cow::Borrowed("d"),
            &MessageItem::Array(_, ref s) => Cow::Owned(format!("a{}", s)),
            &MessageItem::Struct(_) => Cow::Borrowed("r"),
            &MessageItem::Variant(_) => Cow::Borrowed("v"),
            &MessageItem::DictEntry(ref k, ref v) => Cow::Owned(format!("{{{}{}}}", k.type_sig(), v.type_sig())),
            &MessageItem::ObjectPath(_) => Cow::Borrowed("o"),
            &MessageItem::UnixFd(_) => Cow::Borrowed("h"),
        }
    }

    pub fn array_type(&self) -> i32 {
        let s = match self {
            &MessageItem::Str(_) => ffi::DBUS_TYPE_STRING,
            &MessageItem::Bool(_) => ffi::DBUS_TYPE_BOOLEAN,
            &MessageItem::Byte(_) => ffi::DBUS_TYPE_BYTE,
            &MessageItem::Int16(_) => ffi::DBUS_TYPE_INT16,
            &MessageItem::Int32(_) => ffi::DBUS_TYPE_INT32,
            &MessageItem::Int64(_) => ffi::DBUS_TYPE_INT64,
            &MessageItem::UInt16(_) => ffi::DBUS_TYPE_UINT16,
            &MessageItem::UInt32(_) => ffi::DBUS_TYPE_UINT32,
            &MessageItem::UInt64(_) => ffi::DBUS_TYPE_UINT64,
            &MessageItem::Double(_) => ffi::DBUS_TYPE_DOUBLE,
            &MessageItem::Array(_,_) => ffi::DBUS_TYPE_ARRAY,
            &MessageItem::Struct(_) => ffi::DBUS_TYPE_STRUCT,
            &MessageItem::Variant(_) => ffi::DBUS_TYPE_VARIANT,
            &MessageItem::DictEntry(_,_) => ffi::DBUS_TYPE_DICT_ENTRY,
            &MessageItem::ObjectPath(_) => ffi::DBUS_TYPE_OBJECT_PATH,
            &MessageItem::UnixFd(_) => ffi::DBUS_TYPE_UNIX_FD,
        };
        s as i32
    }

    /// Creates an Array<String, Variant> from an iterator with Result passthrough (an Err will abort and return that Err)
    pub fn from_dict<E, I: Iterator<Item=Result<(String, MessageItem),E>>>(i: I) -> Result<MessageItem,E> {
        let mut v = Vec::new();
        for r in i {
            let (s, vv) = try!(r);
            v.push(MessageItem::DictEntry(Box::new(MessageItem::Str(s)), Box::new(MessageItem::Variant(
                Box::new(vv)))));
        }
        Ok(MessageItem::Array(v, Cow::Borrowed("{sv}")))
    }

    /// Creates an MessageItem::Array from a list of MessageItems.
    /// Note: Will panic if the vec is empty or if there are different types in the array
    pub fn new_array(v: Vec<MessageItem>) -> MessageItem {
        let t = v[0].type_sig();
        for i in &v { debug_assert!(i.type_sig() == t) };
        MessageItem::Array(v, t)
    }

    fn from_iter(i: &mut ffi::DBusMessageIter) -> Vec<MessageItem> {
        let mut v = Vec::new();
        loop {
            let t = unsafe { ffi::dbus_message_iter_get_arg_type(i) };
            match t {
                ffi::DBUS_TYPE_INVALID => { return v },
                ffi::DBUS_TYPE_DICT_ENTRY => {
                    let mut subiter = new_dbus_message_iter();
                    unsafe { ffi::dbus_message_iter_recurse(i, &mut subiter) };
                    let a = MessageItem::from_iter(&mut subiter);
                    if a.len() != 2 { panic!("D-Bus dict entry error"); }
                    let mut a = a.into_iter();
                    let key = Box::new(a.next().unwrap());
                    let value = Box::new(a.next().unwrap());
                    v.push(MessageItem::DictEntry(key, value));
                }
                ffi::DBUS_TYPE_VARIANT => {
                    let mut subiter = new_dbus_message_iter();
                    unsafe { ffi::dbus_message_iter_recurse(i, &mut subiter) };
                    let a = MessageItem::from_iter(&mut subiter);
                    if a.len() != 1 { panic!("D-Bus variant error"); }
                    v.push(MessageItem::Variant(Box::new(a.into_iter().next().unwrap())));
                }
                ffi::DBUS_TYPE_ARRAY => {
                    let mut subiter = new_dbus_message_iter();
                    unsafe { ffi::dbus_message_iter_recurse(i, &mut subiter) };
                    let a = MessageItem::from_iter(&mut subiter);
                    let t = if a.len() > 0 { a[0].type_sig() } else {
                        let c = unsafe { ffi::dbus_message_iter_get_signature(&mut subiter) };
                        let s = c_str_to_slice(&(c as *const libc::c_char)).unwrap().to_string();
                        unsafe { ffi::dbus_free(c as *mut libc::c_void) };
                        Cow::Owned(s)
                    };
                    v.push(MessageItem::Array(a, t));
                },
                ffi::DBUS_TYPE_STRUCT => {
                    let mut subiter = new_dbus_message_iter();
                    unsafe { ffi::dbus_message_iter_recurse(i, &mut subiter) };
                    v.push(MessageItem::Struct(MessageItem::from_iter(&mut subiter)));
                },
                ffi::DBUS_TYPE_STRING => {
                    let mut c: *const libc::c_char = ptr::null();
                    unsafe {
                        let p: *mut libc::c_void = mem::transmute(&mut c);
                        ffi::dbus_message_iter_get_basic(i, p);
                    };
                    v.push(MessageItem::Str(c_str_to_slice(&c).expect("D-Bus string error").to_string()));
                },
                ffi::DBUS_TYPE_OBJECT_PATH => {
                    let mut c: *const libc::c_char = ptr::null();
                    unsafe {
                        let p: *mut libc::c_void = mem::transmute(&mut c);
                        ffi::dbus_message_iter_get_basic(i, p);
                    };
                    v.push(MessageItem::ObjectPath(c_str_to_slice(&c).expect("D-Bus object path error").to_string()));
                },
                ffi::DBUS_TYPE_UNIX_FD => v.push(MessageItem::UnixFd(OwnedFd::new(iter_get_basic(i) as libc::c_int))),
                ffi::DBUS_TYPE_BOOLEAN => v.push(MessageItem::Bool((iter_get_basic(i) as u32) != 0)),
                ffi::DBUS_TYPE_BYTE => v.push(MessageItem::Byte(iter_get_basic(i) as u8)),
                ffi::DBUS_TYPE_INT16 => v.push(MessageItem::Int16(iter_get_basic(i) as i16)),
                ffi::DBUS_TYPE_INT32 => v.push(MessageItem::Int32(iter_get_basic(i) as i32)),
                ffi::DBUS_TYPE_INT64 => v.push(MessageItem::Int64(iter_get_basic(i) as i64)),
                ffi::DBUS_TYPE_UINT16 => v.push(MessageItem::UInt16(iter_get_basic(i) as u16)),
                ffi::DBUS_TYPE_UINT32 => v.push(MessageItem::UInt32(iter_get_basic(i) as u32)),
                ffi::DBUS_TYPE_UINT64 => v.push(MessageItem::UInt64(iter_get_basic(i) as u64)),
                ffi::DBUS_TYPE_DOUBLE => v.push(MessageItem::Double(iter_get_f64(i))),

                _ => { panic!("D-Bus unsupported message type {} ({})", t, t as u8 as char); }
            }
            unsafe { ffi::dbus_message_iter_next(i) };
        }
    }

    fn iter_append_basic(&self, i: &mut ffi::DBusMessageIter, v: i64) {
        let t = self.array_type();
        unsafe {
            let p: *const libc::c_void = mem::transmute(&v);
            ffi::dbus_message_iter_append_basic(i, t as libc::c_int, p);
        }
    }

    fn iter_append(&self, i: &mut ffi::DBusMessageIter) {
        match self {
            &MessageItem::Str(ref s) => unsafe {
                let c = to_c_str(s);
                let p = mem::transmute(&c);
                ffi::dbus_message_iter_append_basic(i, ffi::DBUS_TYPE_STRING, p);
            },
            &MessageItem::Bool(b) => self.iter_append_basic(i, b as i64),
            &MessageItem::Byte(b) => self.iter_append_basic(i, b as i64),
            &MessageItem::Int16(b) => self.iter_append_basic(i, b as i64),
            &MessageItem::Int32(b) => self.iter_append_basic(i, b as i64),
            &MessageItem::Int64(b) => self.iter_append_basic(i, b as i64),
            &MessageItem::UInt16(b) => self.iter_append_basic(i, b as i64),
            &MessageItem::UInt32(b) => self.iter_append_basic(i, b as i64),
            &MessageItem::UInt64(b) => self.iter_append_basic(i, b as i64),
            &MessageItem::UnixFd(ref b) => self.iter_append_basic(i, b.as_raw_fd() as i64),
            &MessageItem::Double(b) => iter_append_f64(i, b),
            &MessageItem::Array(ref b, ref t) => iter_append_array(i, &**b, t.clone()),
            &MessageItem::Struct(ref v) => iter_append_struct(i, &**v),
            &MessageItem::Variant(ref b) => iter_append_variant(i, &**b),
            &MessageItem::DictEntry(ref k, ref v) => iter_append_dict(i, &**k, &**v),
            &MessageItem::ObjectPath(ref s) => unsafe {
                let c = to_c_str(s);
                let p = mem::transmute(&c);
                ffi::dbus_message_iter_append_basic(i, ffi::DBUS_TYPE_OBJECT_PATH, p);
            }
        }
    }

    fn copy_to_iter(i: &mut ffi::DBusMessageIter, v: &[MessageItem]) {
        for item in v.iter() {
            item.iter_append(i);
        }
    }
}

/// A D-Bus message. A message contains some headers (e g sender and destination address)
/// and a list of MessageItems.
pub struct Message {
    msg: *mut ffi::DBusMessage,
}

impl Message {
    pub fn new_method_call(destination: &str, path: &str, iface: &str, method: &str) -> Option<Message> {
        init_dbus();
        let (d, p, i, m) = (to_c_str(destination), to_c_str(path), to_c_str(iface), to_c_str(method));
        let ptr = unsafe {
            ffi::dbus_message_new_method_call(d.as_ptr(), p.as_ptr(), i.as_ptr(), m.as_ptr())
        };
        if ptr == ptr::null_mut() { None } else { Some(Message { msg: ptr} ) }
    }

    pub fn new_signal(path: &str, iface: &str, method: &str) -> Option<Message> {
        init_dbus();
        let (p, i, m) = (to_c_str(path), to_c_str(iface), to_c_str(method));
        let ptr = unsafe {
            ffi::dbus_message_new_signal(p.as_ptr(), i.as_ptr(), m.as_ptr())
        };
        if ptr == ptr::null_mut() { None } else { Some(Message { msg: ptr} ) }
    }

    pub fn new_method_return(m: &Message) -> Option<Message> {
        let ptr = unsafe { ffi::dbus_message_new_method_return(m.msg) };
        if ptr == ptr::null_mut() { None } else { Some(Message { msg: ptr} ) }
    }

    pub fn new_error(m: &Message, error_name: &str, error_message: &str) -> Option<Message> {
        let (en, em) = (to_c_str(error_name), to_c_str(error_message));
        let ptr = unsafe { ffi::dbus_message_new_error(m.msg, en.as_ptr(), em.as_ptr()) };
        if ptr == ptr::null_mut() { None } else { Some(Message { msg: ptr} ) }
    }

    pub fn get_items(&mut self) -> Vec<MessageItem> {
        let mut i = new_dbus_message_iter();
        match unsafe { ffi::dbus_message_iter_init(self.msg, &mut i) } {
            0 => Vec::new(),
            _ => MessageItem::from_iter(&mut i)
        }
    }

    pub fn get_serial(&self) -> u32 {
        unsafe { ffi::dbus_message_get_serial(self.msg) }
    }

    pub fn append_items(&mut self, v: &[MessageItem]) {
        let mut i = new_dbus_message_iter();
        unsafe { ffi::dbus_message_iter_init_append(self.msg, &mut i) };
        MessageItem::copy_to_iter(&mut i, v);
    }

    pub fn msg_type(&self) -> MessageType {
        unsafe { mem::transmute(ffi::dbus_message_get_type(self.msg)) }
    }

    pub fn sender(&self) -> Option<String> {
        let s = unsafe { ffi::dbus_message_get_sender(self.msg) };
        c_str_to_slice(&s).map(|s| s.to_string())
    }

    pub fn headers(&self) -> (MessageType, Option<String>, Option<String>, Option<String>) {
        let p = unsafe { ffi::dbus_message_get_path(self.msg) };
        let i = unsafe { ffi::dbus_message_get_interface(self.msg) };
        let m = unsafe { ffi::dbus_message_get_member(self.msg) };
        (self.msg_type(),
         c_str_to_slice(&p).map(|s| s.to_string()),
         c_str_to_slice(&i).map(|s| s.to_string()),
         c_str_to_slice(&m).map(|s| s.to_string()))
    }

    pub fn as_result(&mut self) -> Result<&mut Message, Error> {
        let mut e = Error::empty();
        if unsafe { ffi::dbus_set_error_from_message(e.get_mut(), self.msg) } != 0 { Err(e) }
        else { Ok(self) }
    }
}

impl Drop for Message {
    fn drop(&mut self) {
        unsafe {
            ffi::dbus_message_unref(self.msg);
        }
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "{:?}", self.headers())
    }
}

pub fn message_from_ptr(ptr: *mut ffi::DBusMessage, add_ref: bool) -> Message {
    if add_ref {
        unsafe { ffi::dbus_message_ref(ptr) };
    }
    Message { msg: ptr }
}

pub fn get_message_ptr<'a>(m: &Message) -> *mut ffi::DBusMessage {
    m.msg
}

#[cfg(test)]
mod test {
    extern crate tempdir;

    use super::super::{Connection, ConnectionItem, Message, BusType, MessageItem, OwnedFd, libc};

    #[test]
    fn unix_fd() {
        use std::io::prelude::*;
        use std::io::SeekFrom;
        use std::fs::OpenOptions;
        use std::os::unix::io::AsRawFd;

        let c = Connection::get_private(BusType::Session).unwrap();
        c.register_object_path("/hello").unwrap();
        let mut m = Message::new_method_call(&*c.unique_name(), "/hello", "com.example.hello", "Hello").unwrap();
        let tempdir = tempdir::TempDir::new("dbus-rs-test").unwrap();
        let mut filename = tempdir.path().to_path_buf();
        filename.push("test");
        println!("Creating file {:?}", filename);
        let mut file = OpenOptions::new().create(true).read(true).write(true).open(&filename).unwrap();
        file.write_all(b"z").unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let ofd = OwnedFd::new(file.as_raw_fd());
        m.append_items(&[MessageItem::UnixFd(ofd.clone())]);
        println!("Sending {:?}", m.get_items());
        c.send(m).unwrap();

        for n in c.iter(1000) {
            match n {
                ConnectionItem::MethodCall(mut m) => {
                    if let Some(&MessageItem::UnixFd(ref z)) = m.get_items().get(0) {
                        println!("Got {:?}", m.get_items());
                        let mut q: libc::c_char = 100;
                        assert_eq!(1, unsafe { libc::read(z.as_raw_fd(), &mut q as *mut i8 as *mut libc::c_void, 1) });
                        assert_eq!(q, 'z' as libc::c_char);
                        break;
                    }
                    else {
                        panic!("Expected UnixFd, got {:?}", m.get_items());
                    }
                }
                _ => println!("Got {:?}", n),
            }
        }
    }

    #[test]
    fn message_types() {
        let c = Connection::get_private(BusType::Session).unwrap();
        c.register_object_path("/hello").unwrap();
        let mut m = Message::new_method_call(&*c.unique_name(), "/hello", "com.example.hello", "Hello").unwrap();
        m.append_items(&[
            MessageItem::UInt16(2000),
            MessageItem::new_array(vec!(MessageItem::Byte(129))),
            MessageItem::UInt64(987654321),
            MessageItem::Int32(-1),
            MessageItem::Str(format!("Hello world")),
            MessageItem::Double(-3.14),
            MessageItem::new_array(vec!(
                MessageItem::DictEntry(Box::new(MessageItem::UInt32(123543)), Box::new(MessageItem::Bool(true)))
            ))
        ]);
        let sending = format!("{:?}", m.get_items());
        println!("Sending {}", sending);
        c.send(m).unwrap();

        for n in c.iter(1000) {
            match n {
                ConnectionItem::MethodCall(mut m) => {
                    let receiving = format!("{:?}", m.get_items());
                    println!("Receiving {}", receiving);
                    assert_eq!(sending, receiving);
                    break;
                }
                _ => println!("Got {:?}", n),
            }
        }
    }

    #[test]
    fn dict_of_dicts() {
        use std::collections::BTreeMap;

        let officeactions: BTreeMap<&'static str, MessageItem> = BTreeMap::new();
        let mut officethings = BTreeMap::new();
        officethings.insert("pencil", MessageItem::UInt16(2));
        officethings.insert("paper", MessageItem::UInt16(5));
        let mut homethings = BTreeMap::new();
        homethings.insert("apple", MessageItem::UInt16(11));
        let mut homeifaces = BTreeMap::new();
        homeifaces.insert("getThings", homethings);
        let mut officeifaces = BTreeMap::new();
        officeifaces.insert("getThings", officethings);
        officeifaces.insert("getActions", officeactions);
        let mut paths = BTreeMap::new();
        paths.insert("/hello/office", officeifaces);
        paths.insert("/hello/home", homeifaces);

        println!("Original treemap: {:?}", paths);
        let m = MessageItem::new_array(paths.iter().map(
            |(path, ifaces)| MessageItem::DictEntry(Box::new(MessageItem::ObjectPath(path.to_string())), Box::new(
                MessageItem::new_array(ifaces.iter().map(
                    |(iface, props)| MessageItem::DictEntry(Box::new(MessageItem::Str(iface.to_string())), Box::new(
                        MessageItem::from_dict::<(),_>(props.iter().map(|(name, value)| Ok((name.to_string(), value.clone())))).unwrap()
                    ))
                ).collect())
            ))
        ).collect());
        println!("As MessageItem: {:?}", m);
        assert_eq!(m.type_sig(), "a{oa{sa{sv}}}");

        let c = Connection::get_private(BusType::Session).unwrap();
        c.register_object_path("/hello").unwrap();
        let mut msg = Message::new_method_call(&*c.unique_name(), "/hello", "org.freedesktop.DBusObjectManager", "GetManagedObjects").unwrap();
        msg.append_items(&[m]);
        let sending = format!("{:?}", msg.get_items());
        println!("Sending {}", sending);
        c.send(msg).unwrap();

        for n in c.iter(1000) {
            match n {
                ConnectionItem::MethodCall(mut m) => {
                    let receiving = format!("{:?}", m.get_items());
                    println!("Receiving {}", receiving);
                    assert_eq!(sending, receiving);
                    break;
                }
                _ => println!("Got {:?}", n),
            }
        }
    }
}
