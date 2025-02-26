use crate::abs::{AbsAxis, AbsEvent, AbsInfo};
use crate::convert::Convert;
use crate::evdev::Evdev;
use crate::event::Event;
use crate::glue::{self, input_absinfo};
use crate::key::{Key, KeyEvent};
use crate::rel::{RelAxis, RelEvent};
use crate::uinput::Uinput;

use std::ffi::{CStr, OsStr};
use std::io::Error;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

pub struct Writer {
    uinput: Uinput,
}

impl Writer {
    pub fn builder() -> Result<WriterBuilder, Error> {
        WriterBuilder::new()
    }

    pub async fn write(&mut self, event: &Event) -> Result<(), Error> {
        let (r#type, code, value) = match event {
            Event::Rel(RelEvent { axis, value }) => (glue::EV_REL, axis.to_raw(), Some(*value)),
            Event::Abs(event) => match event {
                AbsEvent::Axis { axis, value } => (glue::EV_ABS, axis.to_raw(), Some(*value)),
                AbsEvent::MtToolType { value } => (
                    glue::EV_ABS,
                    Some(glue::ABS_MT_TOOL_TYPE as _),
                    value.to_raw(),
                ),
            },
            Event::Key(KeyEvent { down, key }) => (glue::EV_KEY, key.to_raw(), Some(*down as _)),
            Event::Sync(event) => (glue::EV_SYN, event.to_raw(), Some(0)),
        };

        if let (Some(code), Some(value)) = (code, value) {
            self.write_raw(r#type as _, code, value).await?;
        }

        Ok(())
    }

    pub fn path(&self) -> Option<&Path> {
        let path = unsafe { glue::libevdev_uinput_get_devnode(self.uinput.as_ptr()) };
        if path.is_null() {
            return None;
        }

        let path = unsafe { CStr::from_ptr(path) };
        let path = OsStr::from_bytes(path.to_bytes());
        let path = Path::new(path);

        Some(path)
    }

    pub(crate) async fn from_evdev(evdev: &Evdev) -> Result<Self, Error> {
        Ok(Self {
            uinput: Uinput::from_evdev(evdev).await?,
        })
    }

    pub(crate) async fn write_raw(
        &mut self,
        r#type: u16,
        code: u16,
        value: i32,
    ) -> Result<(), Error> {
        loop {
            let result = self.uinput.file().writable().await?.try_io(|_| {
                let ret = unsafe {
                    glue::libevdev_uinput_write_event(
                        self.uinput.as_ptr(),
                        r#type as _,
                        code as _,
                        value,
                    )
                };

                if ret < 0 {
                    return Err(Error::from_raw_os_error(-ret).into());
                }

                Ok(())
            });

            match result {
                Ok(result) => return result,
                Err(_) => continue, // This means it would block.
            }
        }
    }
}

pub struct WriterBuilder {
    evdev: Evdev,
}

impl WriterBuilder {
    pub fn new() -> Result<Self, Error> {
        let evdev = Evdev::new()?;

        unsafe {
            glue::libevdev_set_id_bustype(evdev.as_ptr(), glue::BUS_VIRTUAL as _);
        }

        Ok(Self { evdev })
    }

    pub fn name(self, name: &CStr) -> Self {
        unsafe {
            glue::libevdev_set_name(self.evdev.as_ptr(), name.as_ptr());
        }

        self
    }

    pub fn vendor(self, value: u16) -> Self {
        unsafe {
            glue::libevdev_set_id_vendor(self.evdev.as_ptr(), value as _);
        }

        self
    }

    pub fn product(self, value: u16) -> Self {
        unsafe {
            glue::libevdev_set_id_product(self.evdev.as_ptr(), value as _);
        }

        self
    }

    pub fn version(self, value: u16) -> Self {
        unsafe {
            glue::libevdev_set_id_version(self.evdev.as_ptr(), value as _);
        }

        self
    }

    pub fn rel<T: IntoIterator<Item = RelAxis>>(self, items: T) -> Result<Self, Error> {
        for axis in items {
            let axis = match axis.to_raw() {
                Some(axis) => axis,
                None => continue,
            };

            let ret = unsafe {
                glue::libevdev_enable_event_code(
                    self.evdev.as_ptr(),
                    glue::EV_REL,
                    axis as _,
                    ptr::null(),
                )
            };

            if ret < 0 {
                return Err(Error::from_raw_os_error(-ret));
            }
        }

        Ok(self)
    }

    pub fn abs<T: IntoIterator<Item = (AbsAxis, AbsInfo)>>(self, items: T) -> Result<Self, Error> {
        let ret = unsafe {
            glue::libevdev_enable_event_code(
                self.evdev.as_ptr(),
                glue::EV_SYN,
                glue::SYN_MT_REPORT,
                ptr::null(),
            )
        };

        if ret < 0 {
            return Err(Error::from_raw_os_error(-ret));
        }

        for (axis, info) in items {
            let code = match axis.to_raw() {
                Some(code) => code,
                None => continue,
            };

            let info = input_absinfo {
                value: info.min,
                minimum: info.min,
                maximum: info.max,
                fuzz: info.fuzz,
                flat: info.flat,
                resolution: info.resolution,
            };

            let ret = unsafe {
                glue::libevdev_enable_event_code(
                    self.evdev.as_ptr(),
                    glue::EV_ABS,
                    code as _,
                    &info as *const _ as *const _,
                )
            };

            if ret < 0 {
                return Err(Error::from_raw_os_error(-ret));
            }
        }

        Ok(self)
    }

    pub fn key<T: IntoIterator<Item = Key>>(self, items: T) -> Result<Self, Error> {
        for key in items {
            let key = match key.to_raw() {
                Some(key) => key,
                None => continue,
            };

            let ret = unsafe {
                glue::libevdev_enable_event_code(
                    self.evdev.as_ptr(),
                    glue::EV_KEY,
                    key as _,
                    ptr::null(),
                )
            };

            if ret < 0 {
                return Err(Error::from_raw_os_error(-ret));
            }
        }

        Ok(self)
    }

    pub async fn build(self) -> Result<Writer, Error> {
        Writer::from_evdev(&self.evdev).await
    }
}
