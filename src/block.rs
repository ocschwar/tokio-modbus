extern crate modbus;

use modbus::{Coil,binary,Reason,ExceptionCode,tcp};
use {Code, Count, Address, Values, Value, Quantity};
use {ModbusResponsePDU, ModbusRequestPDU}; 
use FunctionCode;
use enum_primitive::FromPrimitive;

pub struct BlankRegisters {
    holding_registers : Vec<u16>,
    input_registers : Vec<u16>,
    coils : Vec<modbus::Coil>,
    discrete_registers : Vec<modbus::Coil>    
}

impl  BlankRegisters {
    
    // An inert implementation, with
    // a data store for each category.
    
    pub fn new () -> BlankRegisters {
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
        if quantity > 0x07B0 {
            ModbusResponsePDU::ModbusErrorResponse{
                code:0x8F,
                exception_code:modbus::ExceptionCode::IllegalDataValue as u8
            }
        } else if address as usize + quantity as usize > self.coils.len() {
            ModbusResponsePDU::ModbusErrorResponse{
                code:0x8F,
                exception_code:modbus::ExceptionCode::IllegalDataAddress as u8
            }            
        } else {
            for i in 0..(quantity as usize) {
                self.coils[ address as usize + i] = values[i] ;
            }
            ModbusResponsePDU::WriteMultipleCoilsResponse {
                code: code , address:address, quantity:quantity
            }
        }
    }

    fn write_multiple_registers(
        & mut self,
        code:Code, address:Address,
        quantity:Quantity, values:Vec<u16>) -> ModbusResponsePDU
    {
        if quantity > 0x007B {
            ModbusResponsePDU::ModbusErrorResponse{
                code:0x90,
                exception_code:modbus::ExceptionCode::IllegalDataValue as u8
            }
        } else if address as usize + quantity as usize > self.holding_registers.len() {
            ModbusResponsePDU::ModbusErrorResponse{
                code:0x90,
                exception_code:modbus::ExceptionCode::IllegalDataAddress as u8
            }            
        } else {
            for i in 0..quantity {
                self.holding_registers[ (address+i) as usize ] = values[i as usize] ;
            }
            ModbusResponsePDU::WriteMultipleRegistersResponse {
                code: code , address:address, quantity:quantity
            }        
        }
    }
    
    fn write_single_coil ( &mut self,code:Code, address:Address, value:Quantity) ->ModbusResponsePDU {
        
        match value {
            0xff00 => {
                self.coils[address as usize] = modbus::Coil::On;
                ModbusResponsePDU::WriteSingleCoilResponse {
                    code: code , address:address, value: value
                }
            },
            0x0000 => {
                self.coils[address as usize] = modbus::Coil::Off;
                ModbusResponsePDU::WriteSingleCoilResponse {
                    code: code , address:address, value: value
                }
            },                    
            _ => ModbusResponsePDU::ModbusErrorResponse{
                code:0x85,
                exception_code:modbus::ExceptionCode::IllegalDataValue as u8
            }
        }
    }
    
    fn write_single_register ( &mut self,code:Code, address:Address, value:Quantity) ->ModbusResponsePDU {
        self.holding_registers[address as usize] = value;
        ModbusResponsePDU::WriteSingleCoilResponse {
            code: code , address:address, value: value
        }
    }
    
    fn read_discrete_inputs (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        if quantity > 2000 {
            ModbusResponsePDU::ModbusErrorResponse{
                code:0x82,
                exception_code:modbus::ExceptionCode::IllegalDataValue as u8
            }
        } else if address as usize + quantity as usize > self.discrete_registers.len() {
            ModbusResponsePDU::ModbusErrorResponse{
                code:0x82,
                exception_code:modbus::ExceptionCode::IllegalDataAddress as u8
            }
            
        } else {
            let values :Vec<u8> = binary::pack_bits(
                &self.discrete_registers[address as usize .. (address +quantity) as usize]);
            ModbusResponsePDU::ReadDiscreteInputsResponse{
                code:code,byte_count: values.len() as u8,input_status:values}
        }
    }
    
    fn read_holding_registers (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        if quantity > 125 {
            ModbusResponsePDU::ModbusErrorResponse{
                code:0x83,
                exception_code:modbus::ExceptionCode::IllegalDataValue as u8
            }
        } else if address as usize + quantity as usize > self.holding_registers.len() {
            ModbusResponsePDU::ModbusErrorResponse{
                code:0x83,
                exception_code:modbus::ExceptionCode::IllegalDataAddress as u8
            }            
        } else {
            let mut values :Vec<u16> = vec![0;quantity as usize];
            println!("quantity {}",quantity);
            values.copy_from_slice(&self.holding_registers[address as usize..address as usize + quantity as usize]);
            ModbusResponsePDU::ReadHoldingRegistersResponse{
                code:code,byte_count: quantity as u8,values:values}        
        }
    }
    
    fn read_input_registers (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        if quantity > 125 {
            ModbusResponsePDU::ModbusErrorResponse{
                code:0x84,
                exception_code:modbus::ExceptionCode::IllegalDataValue as u8
            }
        } else if address as usize + quantity as usize > self.input_registers.len() {
            ModbusResponsePDU::ModbusErrorResponse{
                code:0x84,
                exception_code:modbus::ExceptionCode::IllegalDataAddress as u8
            }            
        } else {
            let mut values :Vec<u16> = vec![0;quantity as usize];
            
            values.copy_from_slice(&self.input_registers[address as usize..address as usize + quantity as usize]);
            ModbusResponsePDU::ReadInputRegistersResponse{
                code:code,byte_count: quantity as u8,values:values}        
        }
    }
    
    fn read_coils (&self,code:Code, address:Address, quantity:Quantity) ->ModbusResponsePDU {
        if quantity > 2000 {
            ModbusResponsePDU::ModbusErrorResponse{
                code:code +0x80,
                exception_code:modbus::ExceptionCode::IllegalDataValue as u8
            }
        } else if address as usize + quantity as usize > self.coils.len() {
            ModbusResponsePDU::ModbusErrorResponse{
                code:code +0x80,
                exception_code:modbus::ExceptionCode::IllegalDataAddress as u8
            }            
        } else {
            let values :Vec<u8> = binary::pack_bits(
                &self.coils[address as usize .. address as usize + quantity as usize]);
            ModbusResponsePDU::ReadCoilsResponse{
                code:code,byte_count: values.len() as u8,coil_status:values}
        }        
    }
    
    pub fn call(& mut self, req: ModbusRequestPDU) -> ModbusResponsePDU {
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
        resp
    }
}
