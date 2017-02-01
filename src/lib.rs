#[macro_use]
extern crate enum_primitive;
extern crate modbus;
extern crate byteorder;
extern crate rustc_serialize;
extern crate tokio_core;
extern crate tokio_proto;

pub mod block ;
pub use block::BlankRegisters;

use tokio_core::io::{Io, Codec, Framed, EasyBuf};

use modbus::{Coil,binary,Reason,ExceptionCode,tcp};
use byteorder::{BigEndian, ReadBytesExt,WriteBytesExt};
use std::io::Cursor;
use tokio_core::reactor::Core;
use tokio_proto::pipeline::ServerProto;

use std::io::{self, ErrorKind, Write,Read};
use enum_primitive::FromPrimitive;

enum_from_primitive! {
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum FunctionCode {
    ReadCoils = 0x01,
    ReadDiscreteInputs = 0x02,
    ReadHoldingRegisters = 0x03,
    ReadInputRegisters = 0x04,
    WriteSingleCoil = 0x05,
    WriteSingleRegister = 0x06,
    WriteMultipleCoils = 0x0f,
    WriteMultipleRegisters = 0x10
}
}


#[derive(Default)]
pub struct ModbusTCPCodec;

type Code = u8;
type Count = u8;
type Address = u16;
type Quantity = u16;
type Value = u16;
type Values = Vec<u16>;


#[derive(Debug)]
pub enum ModbusResponsePDU {
    ReadCoilsResponse{code:Code,byte_count:Count,
                      coil_status: Vec<u8>},
    ReadDiscreteInputsResponse{ code:Code,
                                byte_count: Count,
                                input_status: Vec<u8>},
    ReadHoldingRegistersResponse { code:Code,
                                   byte_count: Count,
                                   values:Values},
    ReadInputRegistersResponse{code:Code,
                               byte_count: Count,
                               values:Values},
    WriteSingleCoilResponse { code:Code,address:Address,value:Value},
    WriteSingleRegisterResponse { code:Code,address:Address,value:Value},
    WriteMultipleCoilsResponse { code:Code, address:Address, quantity:Quantity},
    WriteMultipleRegistersResponse { code:Code, address:Address, quantity:Quantity},
    ModbusErrorResponse {code:Code, exception_code:Code}
}

impl ModbusResponsePDU {
    fn encode (&self) -> Vec<u8> {
        let mut buff:Vec<u8> = Vec::new();        
        match *self {
            ModbusResponsePDU::ReadHoldingRegistersResponse{
                code:c,byte_count:b,
                values: ref v
            } => {
                buff.write_u8(c);
                buff.write_u8(b);
                buff.write(binary::unpack_bytes(v).as_slice());
            },
            ModbusResponsePDU::ReadInputRegistersResponse{
                code:c,byte_count:b,
                values: ref v
            } => {
                buff.write_u8(c);
                buff.write_u8(b);
                buff.write(binary::unpack_bytes(v).as_slice());
            },
            ModbusResponsePDU::ReadDiscreteInputsResponse{
                code:c,byte_count:b,
                input_status:ref s
            } => {
                buff.write_u8(c);
                buff.write_u8(b);
                buff.write(s);
            },
            ModbusResponsePDU::ReadCoilsResponse{
                code:c,byte_count:b,
                coil_status:ref s
            } => {
                buff.write_u8(c);
                buff.write_u8(b);
                buff.write(s);
            },
            ModbusResponsePDU::ModbusErrorResponse{code:c,exception_code:e} => {
                buff.write_u8(c);
                buff.write_u8(e);
            },
            ModbusResponsePDU::WriteMultipleRegistersResponse{
                code:c,address:a,quantity:q } => {
                buff.write_u8(c);
                buff.write_u16::<BigEndian>(a);
                buff.write_u16::<BigEndian>(q);
            },
            ModbusResponsePDU::WriteMultipleCoilsResponse{
                code:c,address:a,quantity:q } => {
                buff.write_u8(c);
                buff.write_u16::<BigEndian>(a);
                buff.write_u16::<BigEndian>(q);
            },
            ModbusResponsePDU::WriteSingleCoilResponse{
                code:c,address:a,value:q } => {
                buff.write_u8(c);
                buff.write_u16::<BigEndian>(a);
                buff.write_u16::<BigEndian>(q);
            },
            ModbusResponsePDU::WriteSingleRegisterResponse{
                code:c,address:a,value:q } => {
                buff.write_u8(c);
                buff.write_u16::<BigEndian>(a);
                buff.write_u16::<BigEndian>(q);
            }
        }
        buff
    }
        

}
// This could be imported from modbus::tcp. 
#[derive(RustcEncodable, RustcDecodable,Debug)]
#[repr(packed)]
pub struct Header {
    tid: u16,
    pid: u16,
    len: u16,
    uid: u8,
}

impl Header {
    fn encode (&self) -> Vec<u8>{
        let mut buff:Vec<u8> = Vec::new();        
        buff.write_u16::<BigEndian>(self.tid);
        buff.write_u16::<BigEndian>(self.pid);
        buff.write_u16::<BigEndian>(self.len);
        buff.write_u8(self.uid);
        buff
    }
}


#[derive(Debug,Clone)]
pub struct ModbusFooter {
    byte_count:u8,
    data : Vec<u8>
}
#[derive(Debug,Clone)]
pub struct ModbusRequestPDU {
    code: u8,
    address: u16,
    // specifies quanity for some instructions,
    // value for others. 
    q_or_v:u16,
    addl: Option<ModbusFooter>
}

#[derive(Debug)]
pub struct ModbusTCPRequest {
    pub header: Header,
    pub pdu: ModbusRequestPDU
}
#[derive(Debug)]
pub struct ModbusTCPResponse {
    pub header: Header,
    pub pdu: ModbusResponsePDU
}

fn parse_mbap (from: &[u8]) -> Header {
    let mut rdr = Cursor::new(from);
    Header{
        tid: rdr.read_u16::<BigEndian>().unwrap(),
        pid: rdr.read_u16::<BigEndian>().unwrap(),
        len: rdr.read_u16::<BigEndian>().unwrap(),
        uid: rdr.read_u8().unwrap(),
    }
}

fn parse_modbus_request_pdu(from: &[u8]) -> ModbusRequestPDU {
    let mut rdr = Cursor::new(from);

    let code = rdr.read_u8().unwrap();
    let address = rdr.read_u16::<BigEndian>().unwrap();
    let count =  rdr.read_u16::<BigEndian>().unwrap();
    let mut addl = None;

    match FunctionCode::from_u8(code).unwrap()  {
        FunctionCode::WriteMultipleCoils  |
        FunctionCode::WriteMultipleRegisters  => {
            let mut buffer = Vec::new();
            let byte_count = rdr.read_u8().unwrap();
            rdr.read_to_end(&mut buffer);
            addl = Some(ModbusFooter{
                byte_count:byte_count,
                data: buffer
            });
            println!("addl {:?}",addl);
        },
        _ =>  {

        }
        
    };
    ModbusRequestPDU{
        code:code as u8,
        address:address,
        q_or_v: count,
        addl:addl
    }
}



impl Codec for ModbusTCPCodec {
    // 
    type In = ModbusTCPRequest;
    type Out = ModbusTCPResponse;

    // Attempt to decode a message from the given buffer if a complete
    // message is available; returns `Ok(None)` if the buffer does not yet
    // hold a complete message.

    // Read first 12 bytes.
    // Decide if more are needed. 
    fn decode(&mut self, buf: &mut EasyBuf) -> std::io::Result<Option<Self::In>> {
        if buf.len() < 12 {
            Ok(None)
        } else {
            let mut length:usize = 0;
            let mut code:u8=0;
            let mut byte_count:usize = 0 ;
            // Scope created just for z so it goes away before we run parse()
            {
                let z = buf.as_slice();
                code = z[7] as u8;
                length = match FunctionCode::from_u8(code).unwrap() {
                    FunctionCode::WriteMultipleCoils |
                    FunctionCode::WriteMultipleRegisters => {
                        if buf.len() == 12 {
                            0;
                        }
                        byte_count = z[12] as usize;
                        if buf.len() >= byte_count + 13 {
                            byte_count+13
                        } else {
                            0
                        }
                    },
                    _ => 12
                }
            }
            let S = &buf.drain_to(length);
            let s = S.as_slice();

            match length {
                0 => Ok(None),

                _ => {
                    Ok(Some(ModbusTCPRequest {
                        header:parse_mbap(&s[0..7]),
                        pdu:parse_modbus_request_pdu(&s[7..length])
                    }))
                }
            }
        }
    }

    // Attempt to decode a message assuming that the given buffer contains
    // *all* remaining input data.
    fn decode_eof(&mut self, buf: &mut EasyBuf) -> io::Result<ModbusTCPRequest> {
        let s = buf.as_slice();
        Ok(ModbusTCPRequest {
            header:parse_mbap(&s[0..7]),
            pdu:parse_modbus_request_pdu(&s[7..buf.len()])
        })
    }

    fn encode(&mut self, item: ModbusTCPResponse, into: &mut Vec<u8>) -> io::Result<()> {
        into.write(item.header.encode().as_slice());
        into.write(item.pdu.encode().as_slice());
        Ok(())
    }
}



pub struct ModbusTCPProto;

impl<T: Io + 'static> ServerProto<T> for ModbusTCPProto {
    type Request = ModbusTCPRequest;
    type Response = ModbusTCPResponse;
    type Transport = Framed<T, ModbusTCPCodec>;
    type BindTransport = ::std::result::Result<Self::Transport,io::Error>;

    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(ModbusTCPCodec))
    }
}
mod test;
