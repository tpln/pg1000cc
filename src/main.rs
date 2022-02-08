extern crate midir;
extern crate simple_error;

use midir::{Ignore, MidiIO, MidiInput, MidiOutput,MidiOutputConnection};
use simple_error::bail;
use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error;
use std::io::{stdin, stdout, Write};

#[derive(Debug, Clone)]
struct ControlMessage {
    cc: u8,
    value: u8,
    channel: u8,
}

impl ControlMessage {
    fn new(cc: u8, value: u8, channel: u8) -> Self {
        Self { cc, value, channel }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut ret = vec![];
        let status: u8 = 0xb | (self.channel & 0b00001111);
        let data1 = self.cc | 0b01111111;
        let data2 = self.value | 0b01111111;
        ret.push(status);
        ret.push(data1);
        ret.push(data2);
        return ret;
    }
}

#[derive(Debug, Clone)]
struct Pg1000SysExMessage {
    id: u8,
    value: u8,
}

impl Pg1000SysExMessage {
    fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        if bytes.len() != 11 {
            bail!("wrong length");
        } else if bytes[0] != 0xf0 {
            bail!("wrong status byte");
        } else {
            Ok(Self {
                id: bytes[7],
                value: bytes[8],
            })
        }
    }
}

struct Mapper {
    seen: HashSet<u8>,
    id_map: HashMap<u8, u8>,
    channel: u8,
    port:MidiOutputConnection
}

impl Mapper {
    // undefined CC's:
    // CC 3
    // CC 9
    // CC 14-15
    // CC 20-31
    // CC 85-90
    // CC 102-119
    const UndefinedControlMessages: &'static [u8] = &[
        3, 9, 14, 15, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 85, 86, 87, 88, 89, 90, 102,
        103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
    ];

    pub fn new(channel:u8, port:MidiOutputConnection) -> Self {
        let mut id_map = HashMap::new();
        id_map.insert(25, 3);
        id_map.insert(24, 9);
        id_map.insert(33, 14);
        id_map.insert(28, 15);
        id_map.insert(35, 20);
        id_map.insert(36, 21);
        id_map.insert(47, 22);
        id_map.insert(22, 23);
        id_map.insert(23, 24);
        id_map.insert(27, 25);
        id_map.insert(30, 26);
        id_map.insert(31, 27);
        id_map.insert(34, 28);
        id_map.insert(43, 30);
        id_map.insert(44, 31);
        id_map.insert(45, 85);
        id_map.insert(32, 86);
        id_map.insert(17, 87);
        id_map.insert(18, 88);
        id_map.insert(19, 89);
        id_map.insert(20, 90);
        id_map.insert(21, 102);

        Self {
            id_map,
            seen: HashSet::new(),
            channel,
            port
        }
    }

    pub fn map(&mut self, message: &[u8]) {
        if let Some(sysex) = Pg1000SysExMessage::from_bytes(message).ok() {
            // To assign PG-1000 sliders to CC's, set discovery_mode to true,
            // run the program and move the sliders you want to use. Then paste
            // the resulting code into new(), build and run.
            let discovery_mode = false;
            if !discovery_mode {
                if self.id_map.contains_key(&sysex.id) {
                    let cc = ControlMessage::new(*self.id_map.get(&sysex.id).unwrap(), sysex.value, self.channel);
                    println!("Sending sysex({}, {}) as cc({}, {}) on channel {}", sysex.id, sysex.value, cc.cc, cc.value, cc.channel);
                    self.port.send(&cc.to_bytes());
                }
            } else {
                if !self.seen.contains(&sysex.id) {
                    self.seen.insert(sysex.id);
                    println!(
                        "id_map.insert({}, {});",
                        sysex.id,
                        Self::UndefinedControlMessages[self.seen.len() - 1]
                    );
                }
            }
        }
    }
}

fn main() {
    match run() {
        Ok(_) => (),
        Err(err) => println!("Error: {}", err),
    }
}

#[cfg(not(target_arch = "wasm32"))] // conn_out is not `Send` in Web MIDI, which means it cannot be passed to connect
fn run() -> Result<(), Box<dyn Error>> {
    let mut midi_in = MidiInput::new("midir forwarding input")?;
    midi_in.ignore(Ignore::None);
    let midi_out = MidiOutput::new("midir forwarding output")?;

    let in_port = select_port(&midi_in, "input")?;
    println!();
    let out_port = select_port(&midi_out, "output")?;

    println!("\nOpening connections");
    let in_port_name = midi_in.port_name(&in_port)?;
    let out_port_name = midi_out.port_name(&out_port)?;

    let mut conn_out = midi_out.connect(&out_port, "midir-forward")?;

    let mut mapper = Mapper::new(1, conn_out);

    // _conn_in needs to be a named parameter, because it needs to be kept alive until the end of the scope
    let _conn_in = midi_in.connect(
        &in_port,
        "midir-forward",
        move |stamp, message, _| {
            mapper.map(message);
            // conn_out
            //     .send(message)
            //     .unwrap_or_else(|_| println!("Error when forwarding message ..."));
            if message.len() == 11 {
                let control_id = message[7];
                let value: i8 = message[8] as i8;
                // println!(
                //     "{}: control_id: {}, value: {}, (len = {})",
                //     stamp,
                //     control_id,
                //     value,
                //     message.len()
                // );
            } else if message.len() != 1 {
                println!(
                    "{}: message: {:?}, (len = {})",
                    stamp,
                    message,
                    message.len()
                );
            }
        },
        (),
    )?;

    println!(
        "Connections open, forwarding from '{}' to '{}' (press enter to exit) ...",
        in_port_name, out_port_name
    );

    let mut input = String::new();
    stdin().read_line(&mut input)?; // wait for next enter key press

    println!("Closing connections");
    Ok(())
}

fn select_port<T: MidiIO>(midi_io: &T, descr: &str) -> Result<T::Port, Box<dyn Error>> {
    println!("Available {} ports:", descr);
    let midi_ports = midi_io.ports();
    for (i, p) in midi_ports.iter().enumerate() {
        println!("{}: {}", i, midi_io.port_name(p)?);
    }
    print!("Please select {} port: ", descr);
    stdout().flush()?;
    let mut input = String::new();
    stdin().read_line(&mut input)?;
    let port = midi_ports
        .get(input.trim().parse::<usize>()?)
        .ok_or("Invalid port number")?;
    Ok(port.clone())
}

#[cfg(target_arch = "wasm32")]
fn run() -> Result<(), Box<dyn Error>> {
    println!("test_forward cannot run on Web MIDI");
    Ok(())
}
