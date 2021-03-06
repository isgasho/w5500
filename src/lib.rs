#![no_std]
#![deny(intra_doc_link_resolution_failure)]

#[macro_use(block)]
extern crate nb;

use byteorder::BigEndian;
use byteorder::ByteOrder;
use embedded_hal::digital::v2::OutputPin;
use embedded_hal::spi::FullDuplex;

const COMMAND_READ: u8 = 0x00 << 2;
const COMMAND_WRITE: u8 = 0x01 << 2;

const VARIABLE_DATA_LENGTH: u8 = 0b_00;
#[allow(unused)]
const FIXED_DATA_LENGTH_1_BYTE: u8 = 0b_01;
#[allow(unused)]
const FIXED_DATA_LENGTH_2_BYTES: u8 = 0b_10;
#[allow(unused)]
const FIXED_DATA_LENGTH_4_BYTES: u8 = 0b_11;

/// IP Address struct.  Represents an IP address as a u8 array of length 4.
/// Can be instantiated with [`IpAddress::new`]
#[derive(Copy, Clone, PartialOrd, PartialEq, Default, Debug)]
pub struct IpAddress {
    pub address: [u8; 4],
}

impl IpAddress {
    /// Instantiate a new IP address with u8s for each address fragment
    pub fn new(a0: u8, a1: u8, a2: u8, a3: u8) -> IpAddress {
        IpAddress {
            address: [a0, a1, a2, a3],
        }
    }
}

impl ::core::fmt::Display for IpAddress {
    /// String formatter for IP addresses, useful for debugging output
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.address[0], self.address[1], self.address[2], self.address[3],
        )
    }
}

/// MAC address struct.  Represents a MAC address as a u8 array of length 6.
/// Can be instantiated with [`MacAddress::new`]
#[derive(Copy, Clone, PartialOrd, PartialEq, Default, Debug)]
pub struct MacAddress {
    pub address: [u8; 6],
}

impl MacAddress {
    /// Instantiate a new MAC address with u8s for each address fragment
    pub fn new(a0: u8, a1: u8, a2: u8, a3: u8, a4: u8, a5: u8) -> MacAddress {
        MacAddress {
            address: [a0, a1, a2, a3, a4, a5],
        }
    }
}

impl ::core::fmt::Display for MacAddress {
    /// String formatter for MAC addresses, useful for debugging output
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.address[0],
            self.address[1],
            self.address[2],
            self.address[3],
            self.address[4],
            self.address[5],
        )
    }
}

/// Error enum that represents the union between SPI hardware errors and digital IO pin errors.
/// Returned as an Error type by many [`ActiveW5500`] operations that talk to the chip
#[derive(Copy, Clone, Debug)]
pub enum TransferError<SpiError, ChipSelectError> {
    SpiError(SpiError),
    ChipSelectError(ChipSelectError),
}

/// Settings for wake on LAN.  Allows the W5500 to optionally emit an interrupt upon receiving a
/// WOL magic packet.
#[derive(Copy, Clone, PartialOrd, PartialEq)]
pub enum OnWakeOnLan {
    InvokeInterrupt,
    Ignore,
}

/// Settings for ping.  Allows the W5500 to respond to or ignore network ping requests.
#[derive(Copy, Clone, PartialOrd, PartialEq)]
pub enum OnPingRequest {
    Respond,
    Ignore,
}

/// Use [`ConnectionType::PPoE`] when talking
/// to an ADSL modem. Otherwise use [`ConnectionType::Ethernet`]
#[derive(Copy, Clone, PartialOrd, PartialEq)]
pub enum ConnectionType {
    PPoE,
    Ethernet,
}

#[derive(Copy, Clone, PartialOrd, PartialEq)]
pub enum ArpResponses {
    Cache,
    DropAfterUse,
}

/// Represents a [`Socket`] that has not yet been initialized for a particular protocol
pub struct UninitializedSocket(Socket);

/// Represents a [`Socket`] that has been initialized to use the UDP protocol
pub struct UdpSocket(Socket);

/// The first level of instantiating communication with the W5500 device. This type is not used
/// for communication, but to keep track of the state of the device. Calling [`W5500::activate`]
/// will return an [`ActiveW5500`] which can be used to communicate with the device. This
/// allows the SPI-Bus to be used for other devices while not being activated without loosing
/// the state.
pub struct W5500<ChipSelect: OutputPin> {
    chip_select: ChipSelect,
    /// each bit represents whether the corresponding socket is available for take
    sockets: u8,
}

impl<ChipSelectError, ChipSelect: OutputPin<Error = ChipSelectError>> W5500<ChipSelect> {
    fn new(chip_select: ChipSelect) -> Self {
        W5500 {
            chip_select,
            sockets: 0xFF,
        }
    }

    /// Creates a new instance and initializes the device accordingly to the parameters.
    /// To do so, it briefly activates the [`W5500`], to set it up with the specified configuration.
    pub fn with_initialisation<Spi: FullDuplex<u8>>(
        chip_select: ChipSelect,
        spi: &mut Spi,
        wol: OnWakeOnLan,
        ping: OnPingRequest,
        mode: ConnectionType,
        arp: ArpResponses,
    ) -> Result<Self, TransferError<Spi::Error, ChipSelectError>> {
        let mut w5500 = Self::new(chip_select);
        {
            let mut w5500_active = w5500.activate(spi)?;
            unsafe {
                // this is safe, since the w5500 instance hast just been created and no sockets
                // are given away or were initialized
                w5500_active.reset()?;
            }
            w5500_active.update_operation_mode(wol, ping, mode, arp)?;
        }
        Ok(w5500)
    }

    /// Returns the requested socket if it is not already taken.
    pub fn take_socket(&mut self, socket: Socket) -> Option<UninitializedSocket> {
        let mask = 0x01 << socket.number();
        if self.sockets & mask == mask {
            self.sockets &= !mask;
            Some(UninitializedSocket(socket))
        } else {
            None
        }
    }

    /// Returns a [`ActiveW5500`] which can be used to modify the device and to communicate
    /// with other ethernet devices within the connected LAN.
    pub fn activate<'a, 'b, Spi: FullDuplex<u8>>(
        &'a mut self,
        spi: &'b mut Spi,
    ) -> Result<ActiveW5500<'a, 'b, ChipSelect, Spi>, TransferError<Spi::Error, ChipSelectError>>
    {
        Ok(ActiveW5500(self, spi))
    }
}

/// This - by concept meant to be a temporary - instance allows to directly communicate with
/// the w5500 device. The reference to the [`W5500`] provides the chip-select [`OutputPin`]
/// as well as its current state. The given SPI interface is borrowed for as long as this
/// instance lives to communicate with the W5500 chip. Drop this instance to re-use the
/// SPI bus for communication with another device.
pub struct ActiveW5500<'a, 'b, ChipSelect: OutputPin, Spi: FullDuplex<u8>>(
    &'a mut W5500<ChipSelect>,
    &'b mut Spi,
);

impl<
        ChipSelectError,
        ChipSelect: OutputPin<Error = ChipSelectError>,
        SpiError,
        Spi: FullDuplex<u8, Error = SpiError>,
    > ActiveW5500<'_, '_, ChipSelect, Spi>
{
    /// Returns the requested socket if it is not already taken. See [`W5500::take_socket`]
    pub fn take_socket(&mut self, socket: Socket) -> Option<UninitializedSocket> {
        self.0.take_socket(socket)
    }

    /// Set up the basic configuration of the W5500 chip
    pub fn update_operation_mode(
        &mut self,
        wol: OnWakeOnLan,
        ping: OnPingRequest,
        mode: ConnectionType,
        arp: ArpResponses,
    ) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        let mut value = 0x00;

        if let OnWakeOnLan::InvokeInterrupt = wol {
            value |= 1 << 5;
        }

        if let OnPingRequest::Ignore = ping {
            value |= 1 << 4;
        }

        if let ConnectionType::PPoE = mode {
            value |= 1 << 3;
        }

        if let ArpResponses::DropAfterUse = arp {
            value |= 1 << 1;
        }

        self.write_to(Register::CommonRegister(0x00_00_u16), &[value])
    }

    /// Sets the IP address of the network gateway (your router's address)
    pub fn set_gateway(
        &mut self,
        gateway: IpAddress,
    ) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        self.write_to(Register::CommonRegister(0x00_01_u16), &gateway.address)
    }

    /// Sets the subnet on the network (for example 255.255.255.0 for /24 subnets)
    pub fn set_subnet(
        &mut self,
        subnet: IpAddress,
    ) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        self.write_to(Register::CommonRegister(0x00_05_u16), &subnet.address)
    }

    /// Sets the MAC address of the W5500 device on the network.
    /// Consider using freely available private/locally administered mac addresses that match the
    /// following hex pattern:
    ///
    /// ```code
    ///  x2-xx-xx-xx-xx-xx
    ///  x6-xx-xx-xx-xx-xx
    ///  xA-xx-xx-xx-xx-xx
    ///  xE-xx-xx-xx-xx-xx
    /// ```
    ///
    /// "Universally administered and locally administered addresses are distinguished by setting
    /// the second-least-significant bit of the first octet of the address" [Wikipedia](https://en.wikipedia.org/wiki/MAC_address#Universal_vs._local)
    ///
    pub fn set_mac(
        &mut self,
        mac: MacAddress,
    ) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        self.write_to(Register::CommonRegister(0x00_09_u16), &mac.address)
    }

    /// Sets the IP address of the W5500 device.  Must be within the range and permitted by the
    /// gateway or the device will not be accessible.
    pub fn set_ip(
        &mut self,
        ip: IpAddress,
    ) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        self.write_to(Register::CommonRegister(0x00_0F_u16), &ip.address)
    }

    /// Reads the 4 bytes from any ip register and returns the value as an [`IpAddress`]
    pub fn read_ip(
        &mut self,
        register: Register,
    ) -> Result<IpAddress, TransferError<SpiError, ChipSelectError>> {
        let mut ip = IpAddress::default();
        self.read_from(register, &mut ip.address)?;
        Ok(ip)
    }

    /// # Safety
    ///
    /// This is unsafe because it cannot set taken [`Sockets`] back to be uninitialized
    /// It assumes, none of the old sockets will used beyond this call. Because the
    /// state of the [`Sockets`] is no longer in sync with the W5500, their usage might
    /// result in undefined behavior.
    ///
    /// [`Sockets`]: crate::Socket
    pub unsafe fn reset(&mut self) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        self.write_to(
            Register::CommonRegister(0x00_00_u16),
            &[
                0b1000_0000, // Mode Register (force reset)
            ],
        )?;
        self.0.sockets = 0xFF;
        Ok(())
    }

    /// TODO document
    fn is_interrupt_set(
        &mut self,
        socket: Socket,
        interrupt: Interrupt,
    ) -> Result<bool, TransferError<SpiError, ChipSelectError>> {
        let mut state = [0u8; 1];
        self.read_from(socket.at(SocketRegister::Interrupt), &mut state)?;
        Ok(state[0] & interrupt as u8 != 0)
    }

    /// TODO document
    pub fn reset_interrupt(
        &mut self,
        socket: Socket,
        interrupt: Interrupt,
    ) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        self.write_to(socket.at(SocketRegister::Interrupt), &[interrupt as u8])
    }

    /// Reads one byte from the given [`Register`] as a u8
    fn read_u8(
        &mut self,
        register: Register,
    ) -> Result<u8, TransferError<SpiError, ChipSelectError>> {
        let mut buffer = [0u8; 1];
        self.read_from(register, &mut buffer)?;
        Ok(buffer[0])
    }

    /// Reads two bytes from the given [`Register`] as a u16
    fn read_u16(
        &mut self,
        register: Register,
    ) -> Result<u16, TransferError<SpiError, ChipSelectError>> {
        let mut buffer = [0u8; 2];
        self.read_from(register, &mut buffer)?;
        Ok(BigEndian::read_u16(&buffer))
    }

    /// Reads enough bytes from the given [`Register`] address onward to fill the `target` u8 slice
    fn read_from(
        &mut self,
        register: Register,
        target: &mut [u8],
    ) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        self.chip_select()
            .map_err(|error| -> TransferError<SpiError, ChipSelectError> {
                TransferError::ChipSelectError(error)
            })?;
        let mut request = [
            0_u8,
            0_u8,
            register.control_byte() | COMMAND_READ | VARIABLE_DATA_LENGTH,
        ];
        BigEndian::write_u16(&mut request[..2], register.address());
        let result = self
            .write_bytes(&request)
            .and_then(|_| self.read_bytes(target));
        self.chip_deselect()
            .map_err(|error| -> TransferError<SpiError, ChipSelectError> {
                TransferError::ChipSelectError(error)
            })?;
        result.map_err(TransferError::SpiError)
    }

    /// Reads enough bytes over SPI to fill the `target` u8 slice
    fn read_bytes(&mut self, bytes: &mut [u8]) -> Result<(), SpiError> {
        for byte in bytes {
            *byte = self.read()?;
        }
        Ok(())
    }

    /// Reads a single byte over SPI
    fn read(&mut self) -> Result<u8, SpiError> {
        // SPI is in read/write sync, for every byte one wants to read, a byte needs
        // to be written
        block!(self.1.send(0x00))?;
        block!(self.1.read())
    }

    /// Write a single u8 byte to the given [`Register`]
    fn write_u8(
        &mut self,
        register: Register,
        value: u8,
    ) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        self.write_to(register, &[value])
    }

    /// Write a u16 as two bytes o the given [`Register`]
    fn write_u16(
        &mut self,
        register: Register,
        value: u16,
    ) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        let mut data = [0u8; 2];
        BigEndian::write_u16(&mut data, value);
        self.write_to(register, &data)
    }

    /// Write a slice of u8 bytes to the given [`Register`]
    fn write_to(
        &mut self,
        register: Register,
        data: &[u8],
    ) -> Result<(), TransferError<SpiError, ChipSelectError>> {
        self.chip_select()
            .map_err(|error| -> TransferError<SpiError, ChipSelectError> {
                TransferError::ChipSelectError(error)
            })?;
        let mut request = [
            0_u8,
            0_u8,
            register.control_byte() | COMMAND_WRITE | VARIABLE_DATA_LENGTH,
        ];
        BigEndian::write_u16(&mut request[..2], register.address());
        let result = self
            .write_bytes(&request)
            .and_then(|_| self.write_bytes(data));
        self.chip_deselect()
            .map_err(|error| -> TransferError<SpiError, ChipSelectError> {
                TransferError::ChipSelectError(error)
            })?;
        result.map_err(TransferError::SpiError)
    }

    /// Write a slice of u8 bytes over SPI
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), SpiError> {
        for b in bytes {
            self.write(*b)?;
        }
        Ok(())
    }

    /// Write a single byte over SPI
    fn write(&mut self, byte: u8) -> Result<(), SpiError> {
        block!(self.1.send(byte))?;
        // SPI is in read/write sync, for every byte one wants to write, a byte needs
        // to be read
        block!(self.1.read())?;
        Ok(())
    }

    /// Begin a SPI frame by setting the CS signal to low
    fn chip_select(&mut self) -> Result<(), ChipSelectError> {
        self.0.chip_select.set_low()
    }

    /// End a SPI frame by setting the CS signal to high
    fn chip_deselect(&mut self) -> Result<(), ChipSelectError> {
        self.0.chip_select.set_high()
    }
}

pub trait IntoUdpSocket<SpiError> {
    fn try_into_udp_server_socket(self, port: u16) -> Result<UdpSocket, SpiError>
    where
        Self: Sized;
}

impl<ChipSelect: OutputPin, Spi: FullDuplex<u8>> IntoUdpSocket<UninitializedSocket>
    for (
        &mut ActiveW5500<'_, '_, ChipSelect, Spi>,
        UninitializedSocket,
    )
{
    /// Initialize a socket to operate in UDP mode
    fn try_into_udp_server_socket(self, port: u16) -> Result<UdpSocket, UninitializedSocket> {
        let socket = (self.1).0;
        (|| {
            self.0.reset_interrupt(socket, Interrupt::SendOk)?;

            self.0
                .write_u16(socket.at(SocketRegister::LocalPort), port)?;
            self.0.write_to(
                socket.at(SocketRegister::Mode),
                &[
                    Protocol::UDP as u8,       // Socket Mode Register
                    SocketCommand::Open as u8, // Socket Command Register
                ],
            )?;
            Ok(UdpSocket(socket))
        })()
        .map_err(|_: TransferError<Spi::Error, ChipSelect::Error>| UninitializedSocket(socket))
    }
}

/// UDP trait that defines send and receive methods for UDP packets
pub trait Udp {
    type Error;

    fn receive(
        &mut self,
        target_buffer: &mut [u8],
    ) -> Result<Option<(IpAddress, u16, usize)>, Self::Error>;

    fn blocking_send(
        &mut self,
        host: &IpAddress,
        host_port: u16,
        data: &[u8],
    ) -> Result<(), Self::Error>;
}

impl<ChipSelect: OutputPin, Spi: FullDuplex<u8>> Udp
    for (&mut ActiveW5500<'_, '_, ChipSelect, Spi>, &UdpSocket)
{
    type Error = TransferError<Spi::Error, ChipSelect::Error>;

    /// Returns a UDP packet if one is available.  Will return `None` if no UDP packets are in the
    /// socket's buffer
    fn receive(
        &mut self,
        destination: &mut [u8],
    ) -> Result<Option<(IpAddress, u16, usize)>, Self::Error> {
        let (w5500, UdpSocket(socket)) = self;

        if w5500.read_u8(socket.at(SocketRegister::InterruptMask))? & 0x04 == 0 {
            return Ok(None);
        }

        let receive_size = loop {
            let s0 = w5500.read_u16(socket.at(SocketRegister::RxReceivedSize))?;
            let s1 = w5500.read_u16(socket.at(SocketRegister::RxReceivedSize))?;
            if s0 == s1 {
                break s0 as usize;
            }
        };
        if receive_size >= 8 {
            let read_pointer = w5500.read_u16(socket.at(SocketRegister::RxReadPointer))?;

            // |<-- read_pointer                                read_pointer + received_size -->|
            // |Destination IP Address | Destination Port | Byte Size of DATA | Actual DATA ... |
            // |   --- 4 Bytes ---     |  --- 2 Bytes --- |  --- 2 Bytes ---  |      ....       |

            let ip = w5500.read_ip(socket.rx_register_at(read_pointer))?;
            let port = w5500.read_u16(socket.rx_register_at(read_pointer + 4))?;
            let data_length = destination
                .len()
                .min(w5500.read_u16(socket.rx_register_at(read_pointer + 6))? as usize);

            w5500.read_from(
                socket.rx_register_at(read_pointer + 8),
                &mut destination[..data_length],
            )?;

            // reset
            w5500.write_u16(
                socket.at(SocketRegister::RxReadPointer),
                read_pointer + receive_size as u16,
            )?;
            w5500.write_u8(
                socket.at(SocketRegister::Command),
                SocketCommand::Recv as u8,
            )?;

            Ok(Some((ip, port, data_length)))
        } else {
            Ok(None)
        }
    }

    /// Sends a UDP packet to the specified IP and port, and blocks until it is fully sent
    fn blocking_send(
        &mut self,
        host: &IpAddress,
        host_port: u16,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        let (w5500, UdpSocket(socket)) = self;

        {
            let local_port = w5500.read_u16(socket.at(SocketRegister::LocalPort))?;
            let local_port = local_port.to_be_bytes();
            let host_port = host_port.to_be_bytes();

            w5500.write_to(
                socket.at(SocketRegister::LocalPort),
                &[
                    local_port[0],
                    local_port[1], // local port u16
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    0x00,
                    0x00, // destination mac
                    host.address[0],
                    host.address[1],
                    host.address[2],
                    host.address[3], // target IP
                    host_port[0],
                    host_port[1], // destination port (5354)
                ],
            )?;
        }

        let data_length = data.len() as u16;
        {
            let data_length = data_length.to_be_bytes();

            // TODO why write [0x00, 0x00] at TxReadPointer at all?
            // TODO Is TxWritePointer not sufficient enough?
            w5500.write_to(
                socket.at(SocketRegister::TxReadPointer),
                &[0x00, 0x00, data_length[0], data_length[1]],
            )?;
        }

        w5500.write_to(
            socket.tx_register_at(0x00_00),
            &data[..data_length as usize],
        )?;

        w5500.write_to(
            socket.at(SocketRegister::Command),
            &[SocketCommand::Send as u8],
        )?;

        for _ in 0..0xFFFF {
            // wait until sent
            if w5500.is_interrupt_set(*socket, Interrupt::SendOk)? {
                w5500.reset_interrupt(*socket, Interrupt::SendOk)?;
                break;
            }
        }
        // restore listen state
        w5500.write_to(
            socket.at(SocketRegister::Mode),
            &[
                Protocol::UDP as u8,       // Socket Mode Register
                SocketCommand::Open as u8, // Socket Command Register
            ],
        )?;
        Ok(())
    }
}

/// Offset addresses in each socket register
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum SocketRegister {
    Mode = 0x0000,
    Command = 0x0001,
    Interrupt = 0x0002,
    Status = 0x0003,
    LocalPort = 0x0004,
    DestinationMac = 0x0006,
    DestinationIp = 0x000C,
    DestinationPort = 0x0010,
    MaxSegmentSize = 0x0012,
    // Reserved 0x0014
    TypeOfService = 0x0015,
    TimeToLive = 0x0016,
    // Reserved 0x0017 - 0x001D
    ReceiveBuffer = 0x001E,
    TransmitBuffer = 0x001F,
    TxFreeSize = 0x0020,
    TxReadPointer = 0x0022,
    TxWritePointer = 0x0024,
    RxReceivedSize = 0x0026,
    RxReadPointer = 0x0028,
    RxWritePointer = 0x002A,
    InterruptMask = 0x002C,
    FragmentOffset = 0x002D,
    KeepAliveTimer = 0x002F,
    // Reserved 0x0030 - 0xFFFF
}

/// Interrupt state bits
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Interrupt {
    SendOk = 1 << 4,
    Timeout = 1 << 3,
    Received = 1 << 2,
    Disconnected = 1 << 1,
    Connected = 1, // 1 << 0
}

/// Register protocol mode bits
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Protocol {
    TCP = 0b0001,
    UDP = 0b0010,
    MACRAW = 0b0100,
}

/// Bits for socket commands
#[repr(u8)]
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum SocketCommand {
    Open = 0x01,
    Listen = 0x02,
    Connect = 0x04,
    Disconnect = 0x08,
    Close = 0x10,
    Send = 0x20,
    SendMac = 0x21,
    SendKeep = 0x22,
    Recv = 0x40,
}

/// Identifiers for each socket on the W5500
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
pub enum Socket {
    Socket0,
    Socket1,
    Socket2,
    Socket3,
    Socket4,
    Socket5,
    Socket6,
    Socket7,
}

impl Socket {
    /// Gets the number of any given socket
    pub fn number(self) -> usize {
        match self {
            Socket::Socket0 => 0,
            Socket::Socket1 => 1,
            Socket::Socket2 => 2,
            Socket::Socket3 => 3,
            Socket::Socket4 => 4,
            Socket::Socket5 => 5,
            Socket::Socket6 => 6,
            Socket::Socket7 => 7,
        }
    }

    /// Returns the register address for a socket instance's TX
    fn tx_register_at(self, address: u16) -> Register {
        match self {
            Socket::Socket0 => Register::Socket0TxBuffer(address),
            Socket::Socket1 => Register::Socket1TxBuffer(address),
            Socket::Socket2 => Register::Socket2TxBuffer(address),
            Socket::Socket3 => Register::Socket3TxBuffer(address),
            Socket::Socket4 => Register::Socket4TxBuffer(address),
            Socket::Socket5 => Register::Socket5TxBuffer(address),
            Socket::Socket6 => Register::Socket6TxBuffer(address),
            Socket::Socket7 => Register::Socket7TxBuffer(address),
        }
    }

    /// Returns the register address for a socket instance's RX
    fn rx_register_at(self, address: u16) -> Register {
        match self {
            Socket::Socket0 => Register::Socket0RxBuffer(address),
            Socket::Socket1 => Register::Socket1RxBuffer(address),
            Socket::Socket2 => Register::Socket2RxBuffer(address),
            Socket::Socket3 => Register::Socket3RxBuffer(address),
            Socket::Socket4 => Register::Socket4RxBuffer(address),
            Socket::Socket5 => Register::Socket5RxBuffer(address),
            Socket::Socket6 => Register::Socket6RxBuffer(address),
            Socket::Socket7 => Register::Socket7RxBuffer(address),
        }
    }

    /// Returns the register address for a socket instance's register
    fn register_at(self, address: u16) -> Register {
        match self {
            Socket::Socket0 => Register::Socket0Register(address),
            Socket::Socket1 => Register::Socket1Register(address),
            Socket::Socket2 => Register::Socket2Register(address),
            Socket::Socket3 => Register::Socket3Register(address),
            Socket::Socket4 => Register::Socket4Register(address),
            Socket::Socket5 => Register::Socket5Register(address),
            Socket::Socket6 => Register::Socket6Register(address),
            Socket::Socket7 => Register::Socket7Register(address),
        }
    }

    fn at(self, register: SocketRegister) -> Register {
        self.register_at(register as u16)
    }
}

/// Chip register names
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Register {
    CommonRegister(u16),

    Socket0Register(u16),
    Socket0TxBuffer(u16),
    Socket0RxBuffer(u16),

    Socket1Register(u16),
    Socket1TxBuffer(u16),
    Socket1RxBuffer(u16),

    Socket2Register(u16),
    Socket2TxBuffer(u16),
    Socket2RxBuffer(u16),

    Socket3Register(u16),
    Socket3TxBuffer(u16),
    Socket3RxBuffer(u16),

    Socket4Register(u16),
    Socket4TxBuffer(u16),
    Socket4RxBuffer(u16),

    Socket5Register(u16),
    Socket5TxBuffer(u16),
    Socket5RxBuffer(u16),

    Socket6Register(u16),
    Socket6TxBuffer(u16),
    Socket6RxBuffer(u16),

    Socket7Register(u16),
    Socket7TxBuffer(u16),
    Socket7RxBuffer(u16),
}

impl Register {
    /// Gets the control bits to identify any given register
    fn control_byte(self) -> u8 {
        #[allow(clippy::inconsistent_digit_grouping)]
        match self {
            Register::CommonRegister(_) => 0b00000_000,

            Register::Socket0Register(_) => 0b00001_000,
            Register::Socket0TxBuffer(_) => 0b00010_000,
            Register::Socket0RxBuffer(_) => 0b00011_000,

            Register::Socket1Register(_) => 0b00101_000,
            Register::Socket1TxBuffer(_) => 0b00110_000,
            Register::Socket1RxBuffer(_) => 0b00111_000,

            Register::Socket2Register(_) => 0b01001_000,
            Register::Socket2TxBuffer(_) => 0b01010_000,
            Register::Socket2RxBuffer(_) => 0b01011_000,

            Register::Socket3Register(_) => 0b01101_000,
            Register::Socket3TxBuffer(_) => 0b01110_000,
            Register::Socket3RxBuffer(_) => 0b01111_000,

            Register::Socket4Register(_) => 0b10001_000,
            Register::Socket4TxBuffer(_) => 0b10010_000,
            Register::Socket4RxBuffer(_) => 0b10011_000,

            Register::Socket5Register(_) => 0b10101_000,
            Register::Socket5TxBuffer(_) => 0b10110_000,
            Register::Socket5RxBuffer(_) => 0b10111_000,

            Register::Socket6Register(_) => 0b11001_000,
            Register::Socket6TxBuffer(_) => 0b11010_000,
            Register::Socket6RxBuffer(_) => 0b11011_000,

            Register::Socket7Register(_) => 0b11101_000,
            Register::Socket7TxBuffer(_) => 0b11110_000,
            Register::Socket7RxBuffer(_) => 0b11111_000,
        }
    }

    /// Returns the associated address as a u16
    fn address(self) -> u16 {
        match self {
            Register::CommonRegister(address) => address,

            Register::Socket0Register(address) => address,
            Register::Socket0TxBuffer(address) => address,
            Register::Socket0RxBuffer(address) => address,

            Register::Socket1Register(address) => address,
            Register::Socket1TxBuffer(address) => address,
            Register::Socket1RxBuffer(address) => address,

            Register::Socket2Register(address) => address,
            Register::Socket2TxBuffer(address) => address,
            Register::Socket2RxBuffer(address) => address,

            Register::Socket3Register(address) => address,
            Register::Socket3TxBuffer(address) => address,
            Register::Socket3RxBuffer(address) => address,

            Register::Socket4Register(address) => address,
            Register::Socket4TxBuffer(address) => address,
            Register::Socket4RxBuffer(address) => address,

            Register::Socket5Register(address) => address,
            Register::Socket5TxBuffer(address) => address,
            Register::Socket5RxBuffer(address) => address,

            Register::Socket6Register(address) => address,
            Register::Socket6TxBuffer(address) => address,
            Register::Socket6RxBuffer(address) => address,

            Register::Socket7Register(address) => address,
            Register::Socket7TxBuffer(address) => address,
            Register::Socket7RxBuffer(address) => address,
        }
    }
}
