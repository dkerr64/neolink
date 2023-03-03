use crate::bc;
use futures::stream::StreamExt;
use log::*;
use serde::{Deserialize, Serialize};
use std::net::ToSocketAddrs;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use Md5Trunc::*;

mod battery;
mod connection;
mod credentials;
mod errors;
mod keepalive;
mod ledstate;
mod login;
mod logout;
mod motion;
mod ping;
mod pirstate;
mod ptz;
mod reboot;
mod resolution;
mod stream;
mod talk;
mod time;
mod version;

pub(crate) use connection::*;
pub(crate) use credentials::*;
pub use errors::Error;
pub use ledstate::LightState;
pub use login::MaxEncryption;
pub use motion::{MotionData, MotionStatus};
pub use pirstate::PirState;
pub use ptz::Direction;
pub use resolution::*;
use std::sync::Arc;
pub use stream::{StreamData, StreamKind};

pub(crate) type Result<T> = std::result::Result<T, Error>;

///
/// This is the primary struct of this library when interacting with the camera
///
pub struct BcCamera {
    channel_id: u8,
    connection: Arc<BcConnection>,
    logged_in: AtomicBool,
    message_num: AtomicU16,
    // Certain commands such as logout require the username/pass in plain text.... why....???
    credentials: Credentials,
}

/// Used to choode the print format of various status messages like battery levels
///
/// Currently this is just the format of battery levels but if we ever got more status
/// messages then they will also use this information
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PrintFormat {
    /// None, don't print
    None,
    /// A human readable output
    Human,
    /// Xml formatted
    Xml,
}

impl BcCamera {
    ///
    /// Create a new camera interface with this address and channel ID
    ///
    /// # Parameters
    ///
    /// * `host` - The address of the camera either ip address or hostname string
    ///
    /// * `channel_id` - The channel ID this is usually zero unless using a NVR
    ///
    /// # Returns
    ///
    /// returns either an error or the camera
    ///
    pub async fn new_with_addr<U: ToSocketAddrs, V: Into<String>, W: Into<String>>(
        host: U,
        channel_id: u8,
        username: V,
        passwd: Option<W>,
        aux_info_format: PrintFormat,
    ) -> Result<Self> {
        let username: String = username.into();
        let passwd: Option<String> = passwd.map(|t| t.into());
        let addr_iter = match host.to_socket_addrs() {
            Ok(iter) => iter,
            Err(_) => return Err(Error::AddrResolutionError),
        };
        for addr in addr_iter {
            if let Ok(cam) = Self::new(
                SocketAddrOrUid::SocketAddr(addr),
                channel_id,
                &username,
                passwd.as_ref(),
                aux_info_format,
            )
            .await
            {
                return Ok(cam);
            }
        }

        Err(Error::CannotInitCamera)
    }

    ///
    /// Create a new camera interface with this uid and channel ID
    ///
    /// # Parameters
    ///
    /// * `uid` - The uid of the camera
    ///
    /// * `channel_id` - The channel ID this is usually zero unless using a NVR
    ///
    /// # Returns
    ///
    /// returns either an error or the camera
    ///
    pub async fn new_with_uid<U: Into<String>, V: Into<String>>(
        uid: &str,
        channel_id: u8,
        username: U,
        passwd: Option<V>,
        discovery_method: DiscoveryMethods,
        aux_info_format: PrintFormat,
    ) -> Result<Self> {
        Self::new(
            SocketAddrOrUid::Uid(uid.to_string(), None, discovery_method),
            channel_id,
            username,
            passwd,
            aux_info_format,
        )
        .await
    }

    ///
    /// Create a new camera interface with this uid and channel ID
    /// while also including IP address information. This will allow
    /// More ways of connecting to a UID camera without contacting
    /// reolink
    ///
    /// # Parameters
    ///
    /// * `uid` - The uid of the camera
    ///
    /// * `address` - The address of the camera
    ///
    /// * `channel_id` - The channel ID this is usually zero unless using a NVR
    ///
    /// # Returns
    ///
    /// returns either an error or the camera
    ///
    pub async fn new_with_uid_and_addr<U: Into<String>, V: Into<String>, W: ToSocketAddrs>(
        uid: &str,
        host: W,
        channel_id: u8,
        username: U,
        passwd: Option<V>,
        discovery_method: DiscoveryMethods,
        aux_info_format: PrintFormat,
    ) -> Result<Self> {
        Self::new(
            SocketAddrOrUid::Uid(
                uid.to_string(),
                Some(host.to_socket_addrs()?.collect()),
                discovery_method,
            ),
            channel_id,
            username,
            passwd,
            aux_info_format,
        )
        .await
    }

    ///
    /// Create a new camera interface with this address/uid and channel ID
    ///
    /// This method will first perform hostname resolution on the address
    /// then fallback to uid if that resolution fails.
    ///
    /// Be aware that it is possible (although unlikely) that there is
    /// a dns entry with the same address as the uid. If uncertain use
    /// one of the other methods.
    ///
    /// # Parameters
    ///
    /// * `host` - The address of the camera either ip address, hostname string, or uid
    ///
    /// * `channel_id` - The channel ID this is usually zero unless using a NVR
    ///
    /// # Returns
    ///
    /// returns either an error or the camera
    ///
    pub async fn new_with_addr_or_uid<U: ToSocketAddrsOrUid, V: Into<String>, W: Into<String>>(
        host: U,
        channel_id: u8,
        username: V,
        passwd: Option<W>,
        aux_info_format: PrintFormat,
    ) -> Result<Self> {
        let addr_iter = match host.to_socket_addrs_or_uid() {
            Ok(iter) => iter,
            Err(_) => return Err(Error::AddrResolutionError),
        };
        let username: String = username.into();
        let passwd: Option<String> = passwd.map(|t| t.into());
        for addr_or_uid in addr_iter {
            if let Ok(cam) = Self::new(
                addr_or_uid,
                channel_id,
                &username,
                passwd.as_ref(),
                aux_info_format,
            )
            .await
            {
                return Ok(cam);
            }
        }

        Err(Error::CannotInitCamera)
    }

    ///
    /// Create a new camera interface with this address/uid and channel ID
    ///
    /// # Parameters
    ///
    /// * `addr` - An enum of [`SocketAddrOrUid`] that contains the address
    ///
    /// * `channel_id` - The channel ID this is usually zero unless using a NVR
    ///
    /// * `username` - The username to login with
    ///
    /// * `passed` - The password to login with required for AES encrypted camera
    ///
    /// # Returns
    ///
    /// returns either an error or the camera
    ///
    pub async fn new<U: Into<String>, V: Into<String>>(
        addr: SocketAddrOrUid,
        channel_id: u8,
        username: U,
        passwd: Option<V>,
        aux_info_format: PrintFormat,
    ) -> Result<Self> {
        let username: String = username.into();
        let passwd: Option<String> = passwd.map(|t| t.into());

        let (sink, source): (BcConnSink, BcConnSource) = match addr {
            SocketAddrOrUid::SocketAddr(addr) => {
                trace!("Trying address {}", addr);
                let (x, r) = TcpSource::new(addr, &username, passwd.as_ref())
                    .await?
                    .split();
                (Box::new(x), Box::new(r))
            }
            SocketAddrOrUid::Uid(uid, optional_addrs, method) => {
                trace!("Trying uid {}", uid);
                // TODO Make configurable
                let (allow_local, allow_remote, allow_map, allow_relay) = match method {
                    DiscoveryMethods::None => (false, false, false, false),
                    DiscoveryMethods::Local => (true, false, false, false),
                    DiscoveryMethods::Remote => (true, true, false, false),
                    DiscoveryMethods::Map => (true, true, true, false),
                    DiscoveryMethods::Relay => (true, true, true, true),
                    DiscoveryMethods::Debug => (false, false, false, true),
                };

                let discovery = {
                    let mut set = tokio::task::JoinSet::new();
                    if allow_local {
                        let uid_local = uid.clone();
                        set.spawn(async move {
                            trace!("Starting Local discovery");
                            let result = Discovery::local(&uid_local, optional_addrs).await;
                            if let Ok(disc) = &result {
                                info!(
                                    "Local discovery success {} at {}",
                                    uid_local,
                                    disc.get_addr()
                                );
                            }
                            result
                        });
                    }
                    if allow_remote {
                        let uid_remote = uid.clone();
                        set.spawn(async move {
                            trace!("Starting Remote discovery");
                            let result = Discovery::remote(&uid_remote).await;
                            if let Ok(disc) = &result {
                                info!(
                                    "Remote discovery success {} at {}",
                                    uid_remote,
                                    disc.get_addr()
                                );
                            }
                            result
                        });
                    }
                    if allow_map {
                        let uid_relay = uid.clone();
                        set.spawn(async move {
                            trace!("Starting Map");
                            let result = Discovery::map(&uid_relay).await;
                            if let Ok(disc) = &result {
                                info!("Map success {} at {}", uid_relay, disc.get_addr());
                            }
                            result
                        });
                    }
                    if allow_relay {
                        let uid_relay = uid.clone();
                        set.spawn(async move {
                            trace!("Starting Relay");
                            let result = Discovery::relay(&uid_relay).await;
                            if let Ok(disc) = &result {
                                info!("Relay success {} at {}", uid_relay, disc.get_addr());
                            }
                            result
                        });
                    }

                    let last_result;
                    loop {
                        match set.join_next().await {
                            Some(Ok(Ok(disc))) => {
                                last_result = Ok(disc);
                                break;
                            }
                            Some(Ok(Err(e))) => {
                                debug!("Discovery Error: {:?}", e);
                            }
                            Some(Err(join_error)) => {
                                last_result = Err(Error::OtherString(format!(
                                    "Panic while joining Discovery threads: {:?}",
                                    join_error
                                )));
                                break;
                            }
                            None => {
                                last_result = Err(Error::DiscoveryTimeout);
                                break;
                            }
                        }
                    }
                    last_result
                }?;
                let (x, r) = UdpSource::new_from_discovery(discovery, &username, passwd.as_ref())
                    .await?
                    .split();
                (Box::new(x), Box::new(r))
            }
        };

        let conn = BcConnection::new(sink, source).await?;

        trace!("Success");
        let me = Self {
            connection: Arc::new(conn),
            message_num: AtomicU16::new(0),
            channel_id,
            logged_in: AtomicBool::new(false),
            credentials: Credentials::new(username, passwd),
        };
        me.keepalive().await?;
        me.monitor_battery(aux_info_format).await?;
        Ok(me)
    }

    /// This method will get a new message number and increment the message count atomically
    pub fn new_message_num(&self) -> u16 {
        self.message_num.fetch_add(1, Ordering::Relaxed)
    }

    fn get_connection(&self) -> Arc<BcConnection> {
        self.connection.clone()
    }

    // Certains commands like logout need the username and password
    // this command will return
    // This will only work after login
    fn get_credentials(&self) -> &Credentials {
        &self.credentials
    }
}

/// The Baichuan library has a very peculiar behavior where it always zeros the last byte.  I
/// believe this is because the MD5'ing of the user/password is a recent retrofit to the code and
/// the original code wanted to prevent a buffer overflow with strcpy.  The modern and legacy login
/// messages have a slightly different behavior; the legacy message has a 32-byte buffer and the
/// modern message uses XML.  The legacy code copies all 32 bytes with memcpy, and the XML value is
/// copied from a C-style string, so the appended null byte is dropped by the XML library - see the
/// test below.
/// Emulate this behavior by providing a configurable mangling of the last character.
#[derive(PartialEq, Eq)]
enum Md5Trunc {
    ZeroLast,
    Truncate,
}

fn md5_string(input: &str, trunc: Md5Trunc) -> String {
    let mut md5 = format!("{:X}\0", md5::compute(input));
    md5.replace_range(31.., if trunc == Truncate { "" } else { "\0" });
    md5
}

#[test]
fn test_md5_string() {
    // Note that these literals are only 31 characters long - see explanation above.
    assert_eq!(
        md5_string("admin", Truncate),
        "21232F297A57A5A743894A0E4A801FC"
    );
    assert_eq!(
        md5_string("admin", ZeroLast),
        "21232F297A57A5A743894A0E4A801FC\0"
    );
}
