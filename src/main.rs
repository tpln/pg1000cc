extern crate midir;
extern crate simple_error;

use midir::{Ignore, MidiIO, MidiInput, MidiOutput,MidiOutputConnection};
use simple_error::bail;
use std::collections::HashMap;
use std::error::Error;
use std::io::{stdin, stdout, Write};
use midir::os::unix::VirtualOutput;

// TODO:
// * read said slider config from a config file (yaml?)

type SysExId = u16;
type CcId = u8;
type MidiValue = i8;

#[derive(Debug, Clone)]
struct MidiRange {
    lo : MidiValue,
    hi : MidiValue,
}

impl MidiRange {
    pub fn new(lo : MidiValue, hi : MidiValue) -> Self {
        Self {
            lo,
            hi
        }
    }

    pub fn width(&self) -> usize {
        (self.hi - self.lo) as usize
    }

    pub fn value_in_other_range(&self, value: MidiValue, other_range: &MidiRange) -> MidiValue {
        let relative = (value - self.lo) as f64 / self.width() as f64;
        other_range.relative_to_absolute(relative)
    }

    pub fn relative_to_absolute(&self, relative :f64) -> MidiValue {
        (self.lo as f64 + relative * self.width() as f64) as MidiValue
    }
}

#[derive(Debug, Clone)]
struct Slider {
    sysex_id : SysExId,
    cc_id : CcId,
    sysex_range : MidiRange,
    cc_range : MidiRange
}

impl Slider {
    pub fn new(sysex_id : SysExId, cc_id : CcId, sysex_range: MidiRange, cc_range: MidiRange) -> Self {
        Self {
            sysex_id,
            cc_id,
            sysex_range,
            cc_range
        }
    }

    pub fn sysex_value_as_cc_value(&self, value: MidiValue) -> MidiValue {
        self.sysex_range.value_in_other_range(value, &self.cc_range)
    }
}

#[derive(Debug, Clone)]
struct ControlMessage {
    cc: CcId,
    value: MidiValue,
    channel: u8,
}

impl ControlMessage {
    fn new(cc: CcId, value: MidiValue, channel: u8) -> Self {
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
        ret.push(data2 as u8);
        return ret;
    }
}

#[derive(Debug, Clone)]
struct Pg1000SysExMessage {
    id: SysExId,
    value: MidiValue,
}

impl Pg1000SysExMessage {
    fn from_bytes(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        if bytes.len() != 11 {
            bail!("wrong length");
        } else if bytes[0] != 0xf0 {
            bail!("wrong status byte");
        } else {
            let id : u16 = (bytes[6] as u16) << 8 | bytes[7] as u16;
            Ok(Self {
                id: id as SysExId,
                value: bytes[8] as MidiValue,
            })
        }
    }
}

struct Mapper {
    sliders: HashMap<SysExId, Slider>,
    channel: u8,
    event_count: u64,
    cc_event_count: u64,
    port:MidiOutputConnection
}

impl Mapper {
    // undefined CC's from MIDI standard:
    const FREE_CCS: &'static [u8] = &[
        3, 9, 14, 15, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 85, 86, 87, 88, 89, 90, 102,
        103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119,
    ];

    pub fn new(channel:u8, port:MidiOutputConnection) -> Self {
        // These are all the sliders on the PG-1000, that have values ranging from 0-100.
        // The rest of the sliders have considerably smaller resolution,
        // ranging e.g. 0-4. Seems their original purpose is to act as
        // switches. I don't have use for those, but if you do, you could add
        // them here as well.
        let mut sliders = HashMap::new();
        let default_cc_range = MidiRange::new(0, 127);
        let default_sysex_range = MidiRange::new(0, 100);

        sliders.insert(0x0319, Slider::new(0x0319, Self::FREE_CCS[0], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0318, Slider::new(0x0318, Self::FREE_CCS[1], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0321, Slider::new(0x0321, Self::FREE_CCS[2], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x031C, Slider::new(0x031C, Self::FREE_CCS[3], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0323, Slider::new(0x0323, Self::FREE_CCS[4], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0324, Slider::new(0x0324, Self::FREE_CCS[5], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x012F, Slider::new(0x012F, Self::FREE_CCS[6], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0116, Slider::new(0x0116, Self::FREE_CCS[7], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0117, Slider::new(0x0117, Self::FREE_CCS[8], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0118, Slider::new(0x0118, Self::FREE_CCS[9], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x011A, Slider::new(0x011A, Self::FREE_CCS[10], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x011B, Slider::new(0x011B, Self::FREE_CCS[11], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x011E, Slider::new(0x011E, Self::FREE_CCS[12], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x011F, Slider::new(0x011F, Self::FREE_CCS[13], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0122, Slider::new(0x0122, Self::FREE_CCS[14], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0123, Slider::new(0x0123, Self::FREE_CCS[15], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x012B, Slider::new(0x012B, Self::FREE_CCS[16], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x012C, Slider::new(0x012C, Self::FREE_CCS[17], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0320, Slider::new(0x0320, Self::FREE_CCS[18], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0111, Slider::new(0x0111, Self::FREE_CCS[19], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0112, Slider::new(0x0112, Self::FREE_CCS[20], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0113, Slider::new(0x0113, Self::FREE_CCS[21], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0113, Slider::new(0x0113, Self::FREE_CCS[21], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0114, Slider::new(0x0114, Self::FREE_CCS[22], default_sysex_range.clone(), default_cc_range.clone()));
        sliders.insert(0x0115, Slider::new(0x0115, Self::FREE_CCS[23], default_sysex_range.clone(), default_cc_range.clone()));

        // T1 - T4
        sliders.insert(0x010D, Slider::new(0x010D, Self::FREE_CCS[24], MidiRange::new(0, 0x32), default_cc_range.clone()));
        sliders.insert(0x010E, Slider::new(0x010E, Self::FREE_CCS[25], MidiRange::new(0, 0x32), default_cc_range.clone()));
        sliders.insert(0x010F, Slider::new(0x010F, Self::FREE_CCS[26], MidiRange::new(0, 0x32), default_cc_range.clone()));
        sliders.insert(0x0110, Slider::new(0x0110, Self::FREE_CCS[27], MidiRange::new(0, 0x32), default_cc_range.clone()));


        

        Self {
            sliders,
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
            self.sliders.get(&sysex.id).map(|slider| {
                let cc = ControlMessage::new(slider.cc_id, slider.sysex_value_as_cc_value(sysex.value), self.channel);
                self.port.send(&cc.to_bytes()).unwrap();
                self.cc_event_count = self.cc_event_count.overflowing_add(1).0;
                println!("Sending sysex({:?}) as cc({:?}) on channel {}", sysex, cc, cc.channel);
                println!("bytes: {:x?}", cc.to_bytes());
            });
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
