//
/*

*/ 
#![feature(inclusive_range_syntax)] 
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

//mod binary;

use std::str;
use std::io::{self, ErrorKind, Write,Read};
use enum_primitive::FromPrimitive;
use futures::{future, Future, BoxFuture};
use tokio_core::io::{Io, Codec, Framed, EasyBuf};
use tokio_proto::TcpServer;
use tokio_proto::pipeline::ServerProto;
use tokio_service::Service;
use std::io::Cursor;
use byteorder::{BigEndian, ReadBytesExt};
use modbus::{Coil,binary,Reason,ExceptionCode,tcp};
use std::thread;
use std::sync::mpsc::channel;
use docopt::Docopt;

use std::sync::mpsc::{Sender, Receiver};

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

/*
        let header = Header::new(self, MODBUS_HEADER_SIZE as u16 + 6u16);
        let mut buff = encode(&header, SizeLimit::Infinite)?;
        buff.write_u8(fun.code())?;
        buff.write_u16::<BigEndian>(addr)?;
        buff.write_u16::<BigEndian>(count)?;
 */
// TODO: Several variants to deal with here.
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

#[derive(RustcEncodable, RustcDecodable,Debug)]
#[repr(packed)]
struct Header {
    tid: u16,
    pid: u16,
    len: u16,
    uid: u8,
}

#[derive(Debug)]
pub struct ModbusFooter {
    byte_count:u8,
    data : Vec<u8>
}
#[derive(Debug)]
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
// TODO: get tcp.rs to have that as a 

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
        let mut rdr = Cursor::new(&buf);
        if buf.len()>= 12 {
            let code = rdr.read_u8()?;
            rdr.set_position(11);
            match FunctionCode::from_u8(code).unwrap() {
                FunctionCode::WriteMultipleCoils |
                FunctionCode::WriteMultipleRegisters => {
                    let byte_count = rdr.read_u8()? as usize;
                    if buf.len() >= byte_count + 13 {
                        Ok(Some(parse_modbus_request(buf.as_slice()
                                                     ).unwrap()))
                    } else {
                        Ok(None)
                    }
                },
                _ => Ok(Some(parse_modbus_request(buf.as_slice()).unwrap()))
            }
            
        } else {
            Ok(None)
        }
    }

    // Attempt to decode a message assuming that the given buffer contains
    // *all* remaining input data.
    fn decode_eof(&mut self, buf: &mut EasyBuf) -> io::Result<ModbusRequest> {
        Ok(parse_modbus_request(&buf.as_slice())?)
    }

    fn encode(&mut self, item: ModbusResponse, into: &mut Vec<u8>) -> io::Result<()> {

        match item {
            _ =>{
                writeln!(into, "{:?}", item);
                Ok(())
            }
        }
    }
}



pub struct ModbusProto;

impl<T: Io + 'static> ServerProto<T> for ModbusProto {
    type Request = ModbusRequest;
    type Response = ModbusResponse;
//    type Error = io::Error;
    type Transport = Framed<T, ModbusCodec>;
    type BindTransport = ::std::result::Result<Self::Transport,io::Error>;

    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(ModbusCodec))
    }
}


// Now we implement a service we'd like to run on top of this protocol

pub struct BlankRegisters {
    HoldingRegisters : Vec<u16>,
    InputRegisters : Vec<u16>,
    Coils : Vec<modbus::Coil>,
    DiscreteRegisters : Vec<modbus::Coil>
    
}

impl  BlankRegisters {
    // An inert implementation, with
    // a data store for each category.
    
    fn new () -> BlankRegisters {
        let mut holding_registers = vec![0;65536];
        let mut coils = vec![modbus::Coil::Off;65536];
        let mut discrete_registers = vec![modbus::Coil::Off;65536];
        let mut input_registers = vec![0;65536];
        BlankRegisters {
            HoldingRegisters: holding_registers,
            Coils:coils,
            InputRegisters:input_registers,
            DiscreteRegisters:discrete_registers
                
        }        
    }
    fn write_multiple_coils(
        & mut self,
        code:Code, address:Address,
        quantity:Quantity, values:Vec<modbus::Coil>) -> ModbusResponsePDU

    {
        for i in 0..values.len() {
            self.Coils[ address as usize + i] = values[i] ;
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
            self.HoldingRegisters[ (address+i) as usize ] = values[i as usize] ;
        }
        ModbusResponsePDU::WriteMultipleRegistersResponse {
            code: code , address:address, quantity:quantity
        }
        
    }
    fn write_single_coil ( &mut self,code:Code, address:Address, value:Quantity) ->ModbusResponsePDU {
        self.Coils[address as usize] = match value {
            0xff00 => modbus::Coil::On,
            0x0000 => modbus::Coil::Off,
            _ => panic!("Damn")
        };
        ModbusResponsePDU::WriteSingleCoilResponse {
            code: code , address:address, value: value
        }
    }
    fn write_single_register ( &mut self,code:Code, address:Address, value:Quantity) ->ModbusResponsePDU {
        self.HoldingRegisters[address as usize] = value;
        ModbusResponsePDU::WriteSingleCoilResponse {
            code: code , address:address, value: value
        }
    }
    fn read_discrete_inputs (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        let values :Vec<u8> = binary::pack_bits(
            &self.Coils[address as usize ... (address +quantity) as usize]);
        ModbusResponsePDU::ReadDiscreteInputsResponse{
            code:code,byte_count: values.len() as u8,input_status:values}
        
    }
    
    fn read_holding_registers (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        let mut values :Vec<u16> = vec![0;quantity as usize];

        values.copy_from_slice(&self.HoldingRegisters[address as usize...(address + quantity) as usize]);
        ModbusResponsePDU::ReadHoldingRegistersResponse{
            code:code,byte_count: quantity as u8,values:values}
        
    }
    fn read_input_registers (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        let mut values :Vec<u16> = vec![0;quantity as usize];

        values.copy_from_slice(&self.InputRegisters[address as usize...(address + quantity) as usize]);
        ModbusResponsePDU::ReadInputRegistersResponse{
            code:code,byte_count: quantity as u8,values:values}
        
    }
    
    fn read_coils (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        let values :Vec<u8> = binary::pack_bits(
            &self.Coils[address as usize ... (address + quantity) as usize]);
        ModbusResponsePDU::ReadCoilsResponse{
            code:code,byte_count: values.len() as u8,coil_status:values}
        
    }
    fn call(& mut self, req: ModbusRequestPDU) -> ModbusResponsePDU {
        let mut resp = match FunctionCode::from_u8(req.code).unwrap(){
            FunctionCode::WriteMultipleCoils  => {
                self.write_multiple_coils(
                    req.code,
                    req.address,
                    req.q_or_v,
                    binary::unpack_bits(
                        &(req.addl.unwrap().data)[...req.q_or_v as usize],
                        req.q_or_v)
                )
            },
            FunctionCode::WriteMultipleRegisters  => {
                self.write_multiple_registers(
                    req.code,
                    req.address,
                    req.q_or_v,
                    binary::pack_bytes(
                        &(req.addl.unwrap().data)[...req.q_or_v as usize])
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
        resp
    }
}

pub struct ModbusService {
    In: Sender<ModbusRequestPDU>,
    out: Receiver<ModbusResponsePDU>,

}

impl ModbusService {
    fn new () -> ModbusService {
        let (In,req_out): (Sender<ModbusRequestPDU>,Receiver<ModbusRequestPDU>)=channel();
        let (resp_in,Out): (Sender<ModbusResponsePDU>,Receiver<ModbusResponsePDU>)=channel();
        let mut block = BlankRegisters::new();
        thread::spawn(move ||{
            block = BlankRegisters::new();
            loop {
                resp_in.send(
                    block.call(req_out.recv().unwrap())).unwrap();
            }
        });
        ModbusService{ In:In,out:Out}
    }
}

impl Service for ModbusService {
    
    type Request = ModbusRequest;
    type Response = ModbusResponse;
    
    type Error = io::Error;
    type Future = BoxFuture<Self::Response, Self::Error>;
 
    fn call(&self, req: Self::Request) -> Self::Future {
        self.In.send(req.pdu).unwrap();
        future::ok( ModbusResponse{
            header:req.header,
            pdu:self.out.recv().unwrap()
        }).boxed()
    }
}

    //WriteMultipleCoils = 0x0f,
    //WriteMultipleRegisters = 0x10
 
// Finally, we can actually host this service locally!
fn main() {

    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| {println!("DAMN {:?}",e); e.exit()});
    println!("{:?}", args);
    
    TcpServer::new(ModbusProto, args.flag_addr.parse().unwrap())
        .serve(|| Ok(ModbusService::new()));
}
