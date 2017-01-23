//
/*

*/ 
#![feature(inclusive_range_syntax)] 
#![feature(type_ascription)]
extern crate futures;

extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate byteorder;
extern crate modbus ;
extern crate docopt;
extern crate rustc_serialize;
#[macro_use]
extern crate enum_primitive;
use std::sync::{Arc,Mutex};
use std::str;
use std::io::{self, ErrorKind, Write,Read};
use enum_primitive::FromPrimitive;
use futures::{future, Future, BoxFuture,Stream,Sink};
use tokio_core::io::{Io, Codec, Framed, EasyBuf};
use tokio_core::reactor::Core;
use tokio_proto::TcpServer;
use tokio_proto::pipeline::ServerProto;
use tokio_service::Service;
use std::io::Cursor;
use byteorder::{BigEndian, ReadBytesExt,WriteBytesExt};
use modbus::{Coil,binary,Reason,ExceptionCode,tcp};
use std::thread;
use std::sync::mpsc::channel;
use docopt::Docopt;


enum_from_primitive! {
#[derive(Copy, Clone, Debug, PartialEq)]
enum FunctionCode {
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

const USAGE: &'static str = "
Usage: slave [options] <resources> ...

Options:
    --addr=<addr>  # Base URL  [default: 127.0.0.1:502].
";

#[derive(Debug, RustcDecodable)]
struct Args {
    arg_resource: Vec<String>,
    flag_addr: String
}


#[derive(Default)]
pub struct ModbusCodec;

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
struct Header {
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
pub struct ModbusRequest {
    header: Header,
    pdu: ModbusRequestPDU
}
#[derive(Debug)]
pub struct ModbusResponse {
    header: Header,
    pdu: ModbusResponsePDU
}

fn parse_modbus_request(from: &[u8]) -> std::io::Result<ModbusRequest> {
    let mut rdr = Cursor::new(from);
    let header = Header{
        tid: rdr.read_u16::<BigEndian>().unwrap(),
        pid: rdr.read_u16::<BigEndian>().unwrap(),
        len: rdr.read_u16::<BigEndian>().unwrap(),
        uid: rdr.read_u8().unwrap(),
    };

    let code = rdr.read_u8().unwrap();
    let address = rdr.read_u16::<BigEndian>()?;
    let count =  rdr.read_u16::<BigEndian>()?;
    let mut addl = None;

    match FunctionCode::from_u8(code).unwrap()  {
        FunctionCode::WriteMultipleCoils  |
        FunctionCode::WriteMultipleRegisters  => {
            let mut buffer = Vec::new();
            let byte_count = rdr.read_u8()?;
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
    Ok(ModbusRequest{
        header:header,
        pdu:ModbusRequestPDU{
            code:code as u8,
            address:address,
            q_or_v: count,
            addl:addl
        }
            
    })
}


impl Codec for ModbusCodec {
    type In = ModbusRequest;
    type Out = ModbusResponse;

    // Attempt to decode a message from the given buffer if a complete
    // message is available; returns `Ok(None)` if the buffer does not yet
    // hold a complete message.

    // Read first 12 bytes.
    // Decide if more are needed. 
    fn decode(&mut self, buf: &mut EasyBuf) -> std::io::Result<Option<ModbusRequest>> {
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

            match length {
                0 => Ok(None),
                _ => Ok(Some(parse_modbus_request(&buf.drain_to(length).as_slice()).unwrap()))
            }
        }
    }

    // Attempt to decode a message assuming that the given buffer contains
    // *all* remaining input data.
    fn decode_eof(&mut self, buf: &mut EasyBuf) -> io::Result<ModbusRequest> {
        Ok(parse_modbus_request(&buf.as_slice())?)
    }

    fn encode(&mut self, item: ModbusResponse, into: &mut Vec<u8>) -> io::Result<()> {
        into.write(item.header.encode().as_slice());
        into.write(item.pdu.encode().as_slice());
        Ok(())
    }
}



pub struct ModbusProto;

impl<T: Io + 'static> ServerProto<T> for ModbusProto {
    type Request = ModbusRequest;
    type Response = ModbusResponse;
    type Transport = Framed<T, ModbusCodec>;
    type BindTransport = ::std::result::Result<Self::Transport,io::Error>;

    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(ModbusCodec))
    }
}


pub struct BlankRegisters {
    holding_registers : Vec<u16>,
    input_registers : Vec<u16>,
    coils : Vec<modbus::Coil>,
    discrete_registers : Vec<modbus::Coil>    
}

impl  BlankRegisters {
    
    // An inert implementation, with
    // a data store for each category.
    
    fn new () -> BlankRegisters {
        let  holding_registers = vec![0;65536];
        let  coils = vec![modbus::Coil::Off;65536];
        let  discrete_registers = vec![modbus::Coil::Off;65536];
        let  input_registers = vec![0;65536];
        BlankRegisters {
            holding_registers: holding_registers,
            coils:coils,
            input_registers:input_registers,
            discrete_registers:discrete_registers                
        }        
    }
    
    fn write_multiple_coils(
        & mut self,
        code:Code, address:Address,
        quantity:Quantity, values:Vec<modbus::Coil>) -> ModbusResponsePDU
    {
        // TODO: this one call is suspect. 
        for i in 0..(quantity as usize) {
            self.coils[ address as usize + i] = values[i] ;
        }
        ModbusResponsePDU::WriteMultipleCoilsResponse {
            code: code , address:address, quantity:quantity
        }
    }

    fn write_multiple_registers(
        & mut self,
        code:Code, address:Address,
        quantity:Quantity, values:Vec<u16>) -> ModbusResponsePDU
    {
        for i in 0..quantity {
            self.holding_registers[ (address+i) as usize ] = values[i as usize] ;
        }
        ModbusResponsePDU::WriteMultipleRegistersResponse {
            code: code , address:address, quantity:quantity
        }        
    }

    fn write_single_coil ( &mut self,code:Code, address:Address, value:Quantity) ->ModbusResponsePDU {
        self.coils[address as usize] = match value {
            0xff00 => modbus::Coil::On,
            0x0000 => modbus::Coil::Off,
            _ => panic!("Damn")
        };
        ModbusResponsePDU::WriteSingleCoilResponse {
            code: code , address:address, value: value
        }
    }
    
    fn write_single_register ( &mut self,code:Code, address:Address, value:Quantity) ->ModbusResponsePDU {
        self.holding_registers[address as usize] = value;
        ModbusResponsePDU::WriteSingleCoilResponse {
            code: code , address:address, value: value
        }
    }
    
    fn read_discrete_inputs (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        let values :Vec<u8> = binary::pack_bits(
            &self.discrete_registers[address as usize .. (address +quantity) as usize]);
        ModbusResponsePDU::ReadDiscreteInputsResponse{
            code:code,byte_count: values.len() as u8,input_status:values}
    }
    
    fn read_holding_registers (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        let mut values :Vec<u16> = vec![0;quantity as usize];
        println!("quantity {}",quantity);
        values.copy_from_slice(&self.holding_registers[address as usize..(address + quantity) as usize]);
        ModbusResponsePDU::ReadHoldingRegistersResponse{
            code:code,byte_count: quantity as u8,values:values}        
    }
    
    fn read_input_registers (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        let mut values :Vec<u16> = vec![0;quantity as usize];

        values.copy_from_slice(&self.input_registers[address as usize..(address + quantity) as usize]);
        ModbusResponsePDU::ReadInputRegistersResponse{
            code:code,byte_count: quantity as u8,values:values}        
    }
    
    fn read_coils (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        let values :Vec<u8> = binary::pack_bits(
            &self.coils[address as usize ... (address + quantity) as usize]);
        ModbusResponsePDU::ReadCoilsResponse{
            code:code,byte_count: values.len() as u8,coil_status:values}
        
    }
    
    fn call(& mut self, req: ModbusRequestPDU) -> ModbusResponsePDU {
        println!("BR call");
        let resp = match FunctionCode::from_u8(req.code).unwrap(){
            FunctionCode::WriteMultipleCoils  => {
                self.write_multiple_coils(
                    req.code,
                    req.address,
                    req.q_or_v,
                    binary::unpack_bits(
                        &(req.addl.unwrap().data),
                        req.q_or_v)
                )
            },
            FunctionCode::WriteMultipleRegisters  => {
                println!("WMR {:?}",req);
                self.write_multiple_registers(
                    req.code,
                    req.address,
                    req.q_or_v,
                    binary::pack_bytes(
                        &(req.addl.unwrap().data))
                        .unwrap()
                )
            },
            FunctionCode::WriteSingleCoil  => {
                self.write_single_coil(
                    req.code,
                    req.address,
                    req.q_or_v)
            },
             FunctionCode::WriteSingleRegister  => {
                self.write_single_register(
                    req.code,
                    req.address,
                    req.q_or_v)
            },
             FunctionCode::ReadHoldingRegisters  => {
                self.read_holding_registers(
                    req.code,
                    req.address,
                    req.q_or_v)
            },
             FunctionCode::ReadInputRegisters  => {
                self.read_input_registers(
                    req.code,
                    req.address,
                    req.q_or_v)
            },
             FunctionCode::ReadCoils  => {
                self.read_coils(
                    req.code,
                    req.address,
                    req.q_or_v)
             },
             FunctionCode::ReadDiscreteInputs  => {
                self.read_discrete_inputs(
                    req.code,
                    req.address,
                    req.q_or_v)
             }
        };
        println!("cresp {:?}",resp);
        resp
    }
}

pub struct ModbusService {
    block:Arc<Mutex<BlankRegisters>>
}

impl ModbusService {
    fn new (
        block:Arc<Mutex<BlankRegisters>>)->ModbusService {
        ModbusService{ block:block}
    }
    
}

impl Service for ModbusService {
    
    type Request = ModbusRequest;
    type Response = ModbusResponse;    
    type Error = io::Error;
    type Future = future::FutureResult<Self::Response, Self::Error>;

    fn call(&self, req: Self::Request) -> Self::Future {
        let mut a = self.block.lock().unwrap();
        future::finished(ModbusResponse {
            header:req.header,
            pdu:
            a.call(req.pdu)
        })
    }
}

fn main() {
    let mut block = Arc::new(Mutex::new(BlankRegisters::new()));

    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| {println!("DAMN {:?}",e); e.exit()});
    println!("{:?}", args);
    
    TcpServer::new(ModbusProto, args.flag_addr.parse().unwrap())
        .serve(move || Ok(ModbusService::new(block.clone())));
}
