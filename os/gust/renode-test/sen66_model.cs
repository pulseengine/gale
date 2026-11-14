//
// A Sensirion SEN66 as a Renode I2C slave, for the gust air-sensor emulation test
// (gale#511). Facts from the SEN6x datasheet, D1 v1.0, October 2026:
//   * commands are 16 bits, MSB first, no CRC byte (§4.6)
//   * every response word is MSB, LSB, CRC-8 (poly 0x31, init 0xFF; §4.9 Table 67)
//   * Get Data Ready 0x0202 -> [0x00, ready, CRC]; false when not measuring (§4.8.3)
//   * Read Measured Values 0x0300 -> 9 words, 27 bytes (§4.8.7 Table 34); the data
//     ready flag resets when the values are read; with no new data the previous
//     values are returned; with no data at all every word is at its upper limit
//   * Start 0x0021, Stop 0x0104
//
// What the model does NOT do: it never NACKs (Renode's I2C slave interface has no way
// to refuse a transfer), it does not model execution times, and it ignores bus timing.
//
// Test control, from the robot script through the monitor (one entry per data-ready
// poll, consumed in order):
//   QueueSample  co2 pm25 voc          a fresh sample (raw datasheet integers)
//   QueueCorrupt co2 pm25 voc word     a fresh sample whose word `word` has a bad CRC
//   QueueNotReady                      one Get Data Ready answers false
//
using System;
using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.I2C;

namespace Antmicro.Renode.Peripherals.Sensors
{
    public class SEN66 : II2CPeripheral
    {
        public SEN66()
        {
            Reset();
        }

        public void Reset()
        {
            queue = new Queue<Entry>();
            lastCommand = 0;
            measuring = false;
            previous = null;
            StartCount = 0;
            ValuesReads = 0;
        }

        public void QueueSample(int co2, int pm25, int voc)
        {
            queue.Enqueue(new Entry { Ready = true, Co2 = co2, Pm25 = pm25, Voc = voc, CorruptWord = -1 });
        }

        public void QueueCorrupt(int co2, int pm25, int voc, int word)
        {
            queue.Enqueue(new Entry { Ready = true, Co2 = co2, Pm25 = pm25, Voc = voc, CorruptWord = word });
        }

        public void QueueNotReady()
        {
            queue.Enqueue(new Entry { Ready = false, CorruptWord = -1 });
        }

        public int StartCount { get; private set; }
        public int ValuesReads { get; private set; }

        public void Write(byte[] data)
        {
            if(data.Length < 2)
            {
                this.Log(LogLevel.Warning, "SEN66: write of {0} byte(s), expected a 2-byte command", data.Length);
                return;
            }
            lastCommand = (data[0] << 8) | data[1];
            this.Log(LogLevel.Info, "SEN66: command 0x{0:X4}", lastCommand);
            switch(lastCommand)
            {
                case 0x0021: measuring = true; StartCount++; break;
                case 0x0104: measuring = false; break;
            }
        }

        public byte[] Read(int count = 1)
        {
            var words = new List<ushort>();
            int corrupt = -1;
            switch(lastCommand)
            {
                case 0x0202:
                {
                    bool ready = false;
                    if(measuring && queue.Count > 0)
                    {
                        if(!queue.Peek().Ready)
                        {
                            queue.Dequeue();
                        }
                        else
                        {
                            ready = true;
                        }
                    }
                    words.Add((ushort)(ready ? 0x0001 : 0x0000));
                    break;
                }
                case 0x0300:
                {
                    ValuesReads++;
                    ushort[] frame;
                    if(queue.Count > 0 && queue.Peek().Ready)
                    {
                        var e = queue.Dequeue();
                        frame = new ushort[] { 50, (ushort)e.Pm25, 130, 140, 4500, 5000, (ushort)e.Voc, 10, (ushort)e.Co2 };
                        corrupt = e.CorruptWord;
                        previous = frame;
                    }
                    else if(previous != null)
                    {
                        frame = previous;
                    }
                    else
                    {
                        frame = new ushort[] { 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0x7FFF, 0x7FFF, 0x7FFF, 0x7FFF, 0xFFFF };
                    }
                    words.AddRange(frame);
                    break;
                }
                default:
                    this.Log(LogLevel.Warning, "SEN66: read after command 0x{0:X4}, which this model does not answer", lastCommand);
                    break;
            }

            var bytes = new List<byte>();
            for(int i = 0; i < words.Count; i++)
            {
                byte hi = (byte)(words[i] >> 8), lo = (byte)words[i];
                byte crc = Crc8(hi, lo);
                if(i == corrupt)
                {
                    crc ^= 0x01;
                }
                bytes.Add(hi);
                bytes.Add(lo);
                bytes.Add(crc);
            }
            var result = new byte[count];
            for(int i = 0; i < count; i++)
            {
                result[i] = i < bytes.Count ? bytes[i] : (byte)0xFF;
            }
            return result;
        }

        public void FinishTransmission()
        {
        }

        public static byte Crc8(byte msb, byte lsb)
        {
            byte crc = 0xFF;
            foreach(var b in new[] { msb, lsb })
            {
                crc ^= b;
                for(int bit = 0; bit < 8; bit++)
                {
                    crc = (byte)((crc & 0x80) != 0 ? (crc << 1) ^ 0x31 : crc << 1);
                }
            }
            return crc;
        }

        private class Entry
        {
            public bool Ready;
            public int Co2;
            public int Pm25;
            public int Voc;
            public int CorruptWord;
        }

        private Queue<Entry> queue;
        private int lastCommand;
        private bool measuring;
        private ushort[] previous;
    }
}
