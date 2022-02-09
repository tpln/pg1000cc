extern crate midir;
extern crate simple_error;

use midir::{Ignore, MidiIO, MidiInput, MidiOutput,MidiOutputConnection};
use simple_error::bail;
use std::collections::HashMap;
use std::error::Error;
use std::io::{stdin, stdout, Write};
use midir::os::unix::VirtualOutput;

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
        // Besides the MIDI standard, here's a convenient page describing
        // the protocol: https://www.songstuff.com/recording/article/midi_message_format/
        let mut ret = vec![];
        let status: u8 = 0xb0 | (self.channel & 0b00001111);
        let data1 = self.cc & 0b01111111;
        let data2 = self.value & 0b01111111;
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
    id_map: HashMap<u8, u8>, // key = sysex id, value = cc number
    channel: u8,
    event_count: u64,
    cc_event_count: u64,
    port:MidiOutputConnection
}

impl Mapper {
    // undefined CC's from MIDI standard:
    // const UNDEFINED_CONTROL_MESSAGES: &'static [u8] = &[
    //     3, 9, 14, 15, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 85, 86, 87, 88, 89, 90, 102,
    //     103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
    // ];

    pub fn new(channel:u8, port:MidiOutputConnection) -> Self {
        let mut id_map = HashMap::new();

        // These are all the sliders on the PG-1000, that have values ranging from 0-100.
        // The rest of the sliders have considerably smaller resolution,
        // ranging e.g. 0-4. Seems their original purpose is to act as
        // switches. I don't have use for those, but if you do, you could add
        // them here as well.
        //
        // The map values are the cc numbers, these specifically are unused
        // values from the MIDI-standard.
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
            channel,
            port,
            event_count: 0,
            cc_event_count: 0
        }
    }

    pub fn map(&mut self, message: &[u8]) {
        self.event_count = self.event_count.overflowing_add(1).0;

        // If this is a Roland PG-1000 sysex message and we've got a
        // mapping for it, then map...
        if let Some(sysex) = Pg1000SysExMessage::from_bytes(message).ok() {
            if self.id_map.contains_key(&sysex.id) {
                let cc = ControlMessage::new(*self.id_map.get(&sysex.id).unwrap(), (sysex.value as f64 * 1.27f64) as u8, self.channel);
                //println!("Sending sysex({:?}) as cc({:?}) on channel {}", sysex, cc, cc.channel);
                //println!("bytes: {:x?}", cc.to_bytes());
                self.port.send(&cc.to_bytes()).unwrap();
                self.cc_event_count = self.cc_event_count.overflowing_add(1).0;
            }
        }
        else {
            // ...otherwise passthrough
            self.port.send(message).unwrap();
        }
        //print!("\rTotal events: {}, CCs: {}, last input {:x?}", self.event_count, self.cc_event_count, message);
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
    let mut midi_in = MidiInput::new("pg1000cc forwarding input")?;
    midi_in.ignore(Ignore::None);
    let midi_out = MidiOutput::new("pg1000cc forwarding output")?;

    let in_port = select_port(&midi_in, "input")?;
    println!();
    let conn_out = midi_out.create_virtual("pg1000cc")?;

    println!("\nOpening connections");
    let in_port_name = midi_in.port_name(&in_port)?;

    let mut mapper = Mapper::new(1, conn_out);

    // _conn_in needs to be a named parameter, because it needs to be kept alive until the end of the scope
    let _conn_in = midi_in.connect(
        &in_port,
        "pg1000cc",
        move |_, message, _| {
            mapper.map(message);
        },
        (),
    )?;

    println!(
        "Connections open, forwarding from '{}' to 'pg1000cc' (press enter to exit) ...",
        in_port_name
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
    println!("pg1000cc cannot run on Web MIDI");
    Ok(())
}
