use ihex::Record;
use probe_rs_target::{
    MemoryRange, MemoryRegion, NvmRegion, RawFlashAlgorithm, TargetDescriptionSource,
};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::str::FromStr;

use super::builder::FlashBuilder;
use super::{
    extract_from_elf, BinOptions, DownloadOptions, FileDownloadError, FlashError, Flasher,
    IdfOptions,
};
use crate::memory::MemoryInterface;
use crate::session::Session;
use crate::Target;

/// `FlashLoader` is a struct which manages the flashing of any chunks of data onto any sections of flash.
///
/// Use [add_data()](FlashLoader::add_data) to add a chunk of data.
/// Once you are done adding all your data, use `commit()` to flash the data.
/// The flash loader will make sure to select the appropriate flash region for the right data chunks.
/// Region crossing data chunks are allowed as long as the regions are contiguous.
pub struct FlashLoader {
    memory_map: Vec<MemoryRegion>,
    builder: FlashBuilder,

    /// Source of the flash description,
    /// used for diagnostics.
    source: TargetDescriptionSource,
}

impl FlashLoader {
    /// Create a new flash loader.
    pub fn new(memory_map: Vec<MemoryRegion>, source: TargetDescriptionSource) -> Self {
        Self {
            memory_map,
            builder: FlashBuilder::new(),
            source,
        }
    }

    /// Check the given address range is completely covered by the memory map,
    /// possibly by multiple memory regions.
    fn check_data_in_memory_map(&mut self, range: Range<u64>) -> Result<(), FlashError> {
        let mut address = range.start;
        while address < range.end {
            match Self::get_region_for_address(&self.memory_map, address) {
                Some(MemoryRegion::Nvm(region)) => address = region.range.end,
                Some(MemoryRegion::Ram(region)) => address = region.range.end,
                _ => {
                    return Err(FlashError::NoSuitableNvm {
                        start: range.start,
                        end: range.end,
                        description_source: self.source.clone(),
                    })
                }
            }
        }
        Ok(())
    }

    /// Stages a chunk of data to be programmed.
    ///
    /// The chunk can cross flash boundaries as long as one flash region connects to another flash region.
    pub fn add_data(&mut self, address: u64, data: &[u8]) -> Result<(), FlashError> {
        tracing::trace!(
            "Adding data at address {:#010x} with size {} bytes",
            address,
            data.len()
        );

        self.check_data_in_memory_map(address..address + data.len() as u64)?;
        self.builder.add_data(address, data)
    }

    pub(super) fn get_region_for_address(
        memory_map: &[MemoryRegion],
        address: u64,
    ) -> Option<&MemoryRegion> {
        for region in memory_map {
            let r = match region {
                MemoryRegion::Ram(r) => r.range.clone(),
                MemoryRegion::Nvm(r) => r.range.clone(),
                MemoryRegion::Generic(r) => r.range.clone(),
            };
            if r.contains(&address) {
                return Some(region);
            }
        }
        None
    }

    /// Reads the data from the binary file and adds it to the loader without splitting it into flash instructions yet.
    pub fn load_bin_data<T: Read + Seek>(
        &mut self,
        file: &mut T,
        options: BinOptions,
    ) -> Result<(), FileDownloadError> {
        // Skip the specified bytes.
        file.seek(SeekFrom::Start(u64::from(options.skip)))?;

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        self.add_data(
            if let Some(address) = options.base_address {
                address
            } else {
                // If no base address is specified use the start of the boot memory.
                // TODO: Implement this as soon as we know targets.
                0
            },
            &buf,
        )?;

        Ok(())
    }

    /// Loads an esp-idf application into the loader by converting the main application to the esp-idf bootloader format,
    /// appending it to the loader along with the bootloader and partition table.
    ///
    /// This does not create and flash loader instructions yet.
    pub fn load_idf_data<T: Read>(
        &mut self,
        session: &mut Session,
        file: &mut T,
        options: IdfOptions,
    ) -> Result<(), FileDownloadError> {
        let target = session.target();
        let chip = espflash::targets::Chip::from_str(&target.name)
            .map_err(|_| FileDownloadError::IdfUnsupported(target.name.clone()))?
            .into_target();

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let firmware = espflash::elf::ElfFirmwareImage::try_from(&buf[..])?;
        let image = chip.get_flash_image(
            &firmware,
            options.bootloader,
            options.partition_table,
            None,
            None,
            None,
            None,
            None,
        )?;
        let parts: Vec<_> = image.flash_segments().collect();

        for data in parts {
            self.add_data(data.addr.into(), &data.data)?;
        }

        Ok(())
    }

    /// Reads the HEX data segments and adds them as loadable data blocks to the loader.
    /// This does not create and flash loader instructions yet.
    pub fn load_hex_data<T: Read + Seek>(&mut self, file: &mut T) -> Result<(), FileDownloadError> {
        let mut base_address = 0;

        let mut data = String::new();
        file.read_to_string(&mut data)?;

        for record in ihex::Reader::new(&data) {
            let record = record?;
            use Record::*;
            match record {
                Data { offset, value } => {
                    let offset = base_address + offset as u64;
                    self.add_data(offset, &value)?;
                }
                EndOfFile => (),
                ExtendedSegmentAddress(address) => {
                    base_address = (address as u64) * 16;
                }
                StartSegmentAddress { .. } => (),
                ExtendedLinearAddress(address) => {
                    base_address = (address as u64) << 16;
                }
                StartLinearAddress(_) => (),
            };
        }
        Ok(())
    }

    /// Prepares the data sections that have to be loaded into flash from an ELF file.
    /// This will validate the ELF file and transform all its data into sections but no flash loader commands yet.
    pub fn load_elf_data<T: Read>(&mut self, file: &mut T) -> Result<(), FileDownloadError> {
        let mut elf_buffer = Vec::new();
        file.read_to_end(&mut elf_buffer)?;

        let mut extracted_data = Vec::new();

        let num_sections = extract_from_elf(&mut extracted_data, &elf_buffer)?;

        if num_sections == 0 {
            tracing::warn!("No loadable segments were found in the ELF file.");
            return Err(FileDownloadError::NoLoadableSegments);
        }

        tracing::info!("Found {} loadable sections:", num_sections);

        for section in &extracted_data {
            let source = if section.section_names.is_empty() {
                "Unknown".to_string()
            } else if section.section_names.len() == 1 {
                section.section_names[0].to_owned()
            } else {
                "Multiple sections".to_owned()
            };

            tracing::info!(
                "    {} at {:#010X?} ({} byte{})",
                source,
                section.address,
                section.data.len(),
                if section.data.len() == 1 { "" } else { "s" }
            );
        }

        for data in extracted_data {
            self.add_data(data.address.into(), data.data)?;
        }

        Ok(())
    }

    /// Writes all the stored data chunks to flash.
    ///
    /// Requires a session with an attached target that has a known flash algorithm.
    ///
    /// If `do_chip_erase` is `true` the entire flash will be erased.
    pub fn commit(
        &self,
        session: &mut Session,
        options: DownloadOptions,
    ) -> Result<(), FlashError> {
        tracing::debug!("committing FlashLoader!");

        tracing::debug!("Contents of builder:");
        for (&address, data) in &self.builder.data {
            tracing::debug!(
                "    data: {:08x}-{:08x} ({} bytes)",
                address,
                address + data.len() as u64,
                data.len()
            );
        }

        tracing::debug!("Flash algorithms:");
        for algorithm in &session.target().flash_algorithms {
            let Range { start, end } = algorithm.flash_properties.address_range;

            tracing::debug!(
                "    algo {}: {:08x}-{:08x} ({} bytes)",
                algorithm.name,
                start,
                end,
                end - start
            );
        }

        // Iterate over all memory regions, and program their data.

        if self.memory_map != session.target().memory_map {
            tracing::warn!("Memory map of flash loader does not match memory map of target!");
        }

        let mut algos: HashMap<(String, String), Vec<NvmRegion>> = HashMap::new();

        // Commit NVM first

        // Iterate all NvmRegions and group them by flash algorithm.
        // This avoids loading the same algorithm twice if it's used for two regions.
        //
        // This also ensures correct operation when chip erase is used. We assume doing a chip erase
        // using a given algorithm erases all regions controlled by it. Therefore, we must do
        // chip erase once per algorithm, not once per region. Otherwise subsequent chip erases will
        // erase previous regions' flashed contents.
        tracing::debug!("Regions:");
        for region in &self.memory_map {
            if let MemoryRegion::Nvm(region) = region {
                tracing::debug!(
                    "    region: {:08x}-{:08x} ({} bytes)",
                    region.range.start,
                    region.range.end,
                    region.range.end - region.range.start
                );

                // If we have no data in this region, ignore it.
                // This avoids uselessly initializing and deinitializing its flash algorithm.
                if !self.builder.has_data_in_range(&region.range) {
                    tracing::debug!("     -- empty, ignoring!");
                    continue;
                }

                let algo = Self::get_flash_algorithm_for_region(region, session.target())?;

                let entry = algos
                    .entry((
                        algo.name.clone(),
                        region
                            .cores
                            .first()
                            .ok_or_else(|| FlashError::NoNvmCoreAccess(region.clone()))?
                            .clone(),
                    ))
                    .or_default();
                entry.push(region.clone());

                tracing::debug!("     -- using algorithm: {}", algo.name);
            }
        }

        if options.dry_run {
            tracing::info!("Skipping programming, dry run!");

            if let Some(progress) = options.progress {
                progress.failed_filling();
                progress.failed_erasing();
                progress.failed_programming();
            }

            return Ok(());
        }

        // Iterate all flash algorithms we need to use.
        for ((algo_name, core_name), regions) in algos {
            tracing::debug!("Flashing ranges for algo: {}", algo_name);

            // This can't fail, algo_name comes from the target.
            let algo = session.target().flash_algorithm_by_name(&algo_name);
            let algo = algo.unwrap().clone();

            let core = session
                .target()
                .cores
                .iter()
                .position(|c| c.name == core_name)
                .unwrap();
            let mut flasher = Flasher::new(session, core, &algo, options.progress.clone())?;

            let mut do_chip_erase = options.do_chip_erase;

            // If the flash algo doesn't support erase all, disable chip erase.
            if do_chip_erase && !flasher.is_chip_erase_supported() {
                do_chip_erase = false;
                tracing::warn!("Chip erase was the selected method to erase the sectors but this chip does not support chip erases (yet).");
                tracing::warn!("A manual sector erase will be performed.");
            }

            if do_chip_erase {
                tracing::debug!("    Doing chip erase...");
                flasher.run_erase_all()?;
            }

            let mut do_use_double_buffering = flasher.double_buffering_supported();
            if do_use_double_buffering && options.disable_double_buffering {
                tracing::info!("Disabled double-buffering support for loader via passed option, though target supports it.");
                do_use_double_buffering = false;
            }

            for region in regions {
                tracing::debug!(
                    "    programming region: {:08x}-{:08x} ({} bytes)",
                    region.range.start,
                    region.range.end,
                    region.range.end - region.range.start
                );

                // Program the data.
                flasher.program(
                    &region,
                    &self.builder,
                    options.keep_unwritten_bytes,
                    do_use_double_buffering,
                    options.skip_erase || do_chip_erase,
                )?;
            }
        }

        tracing::debug!("committing RAM!");

        // Commit RAM last, because NVM flashing overwrites RAM
        for region in &self.memory_map {
            if let MemoryRegion::Ram(region) = region {
                tracing::debug!(
                    "    region: {:08x}-{:08x} ({} bytes)",
                    region.range.start,
                    region.range.end,
                    region.range.end - region.range.start
                );

                let region_core_index = session
                    .target()
                    .core_index_by_name(
                        region
                            .cores
                            .first()
                            .ok_or_else(|| FlashError::NoRamCoreAccess(region.clone()))?,
                    )
                    .unwrap();
                // Attach to memory and core.
                let mut core = session.core(region_core_index).map_err(FlashError::Core)?;

                let mut some = false;
                for (address, data) in self.builder.data_in_range(&region.range) {
                    some = true;
                    tracing::debug!(
                        "     -- writing: {:08x}-{:08x} ({} bytes)",
                        address,
                        address + data.len() as u64,
                        data.len()
                    );
                    // Write data to memory.
                    core.write_8(address, data).map_err(FlashError::Core)?;
                }

                if !some {
                    tracing::debug!("     -- empty.")
                }
            }
        }

        if options.verify {
            tracing::debug!("Verifying!");
            for (&address, data) in &self.builder.data {
                tracing::debug!(
                    "    data: {:08x}-{:08x} ({} bytes)",
                    address,
                    address + data.len() as u64,
                    data.len()
                );

                let associated_region = session
                    .target()
                    .get_memory_region_by_address(address)
                    .unwrap();
                let core_name = match associated_region {
                    MemoryRegion::Ram(r) => &r.cores,
                    MemoryRegion::Generic(r) => &r.cores,
                    MemoryRegion::Nvm(r) => &r.cores,
                }
                .first()
                .unwrap();
                let core_index = session.target().core_index_by_name(core_name).unwrap();
                let mut core = session.core(core_index).map_err(FlashError::Core)?;

                let mut written_data = vec![0; data.len()];
                core.read(address, &mut written_data)
                    .map_err(FlashError::Core)?;

                if data != &written_data {
                    return Err(FlashError::Verify);
                }
            }
        }

        Ok(())
    }

    /// Try to find a flash algorithm for the given NvmRegion.
    /// Errors when:
    /// - there's no algo for the region.
    /// - there's multiple default algos for the region.
    /// - there's multiple fitting algos but no default.
    pub(crate) fn get_flash_algorithm_for_region<'a>(
        region: &NvmRegion,
        target: &'a Target,
    ) -> Result<&'a RawFlashAlgorithm, FlashError> {
        let algorithms = target
            .flash_algorithms
            .iter()
            // filter for algorithims that contiain adress range
            .filter(|&fa| {
                fa.flash_properties
                    .address_range
                    .contains_range(&region.range)
            })
            .collect::<Vec<_>>();

        match algorithms.len() {
            0 => Err(FlashError::NoFlashLoaderAlgorithmAttached {
                name: target.name.clone(),
            }),
            1 => Ok(algorithms[0]),
            _ => {
                // filter for defaults
                let defaults = algorithms
                    .iter()
                    .filter(|&fa| fa.default)
                    .collect::<Vec<_>>();

                match defaults.len() {
                    0 => Err(FlashError::MultipleFlashLoaderAlgorithmsNoDefault {
                        region: region.clone(),
                    }),
                    1 => Ok(defaults[0]),
                    _ => Err(FlashError::MultipleDefaultFlashLoaderAlgorithms {
                        region: region.clone(),
                    }),
                }
            }
        }
    }

    /// Return data chunks stored in the `FlashLoader` as pairs of address and bytes.
    pub fn data(&self) -> impl Iterator<Item = (u64, &[u8])> {
        self.builder
            .data
            .iter()
            .map(|(address, data)| (*address, data.as_slice()))
    }
}
