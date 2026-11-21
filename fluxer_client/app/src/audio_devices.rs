//! OS audio input/output device enumeration via `cpal`.

use cpal::traits::{DeviceTrait as _, HostTrait as _};

/// Enumerated OS audio input and output device names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AudioDeviceLists {
    pub input_devices: Vec<String>,
    pub output_devices: Vec<String>,
}

/// Trims, drops empty names, deduplicates, and sorts device names lexicographically.
pub(crate) fn normalize_device_names<I: IntoIterator<Item = String>>(names: I) -> Vec<String> {
    let mut normalized: Vec<String> = names
        .into_iter()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

/// Enumerates available OS audio input and output devices.
///
/// Returns empty lists gracefully if the audio host or enumeration fails
/// (for example in headless CI or systems without audio hardware).
pub(crate) fn enumerate_audio_devices() -> AudioDeviceLists {
    let host = cpal::default_host();
    let input_devices = host
        .input_devices()
        .map(|devices| {
            normalize_device_names(
                devices.filter_map(|device| device.description().ok().map(|desc| desc.name().to_owned())),
            )
        })
        .unwrap_or_default();
    let output_devices = host
        .output_devices()
        .map(|devices| {
            normalize_device_names(
                devices.filter_map(|device| device.description().ok().map(|desc| desc.name().to_owned())),
            )
        })
        .unwrap_or_default();
    AudioDeviceLists {
        input_devices,
        output_devices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_device_names_trims_drops_empty_dedups_and_sorts() {
        let input = vec![
            "  USB Mic  ".to_owned(),
            "".to_owned(),
            "   ".to_owned(),
            "Built-in Mic".to_owned(),
            "USB Mic".to_owned(),
            "Alpha Mic".to_owned(),
            "\tBuilt-in Mic\n".to_owned(),
        ];
        assert_eq!(
            normalize_device_names(input),
            vec!["Alpha Mic", "Built-in Mic", "USB Mic"]
        );
    }

    #[test]
    fn enumerate_audio_devices_does_not_panic() {
        let lists = enumerate_audio_devices();
        assert_eq!(lists.input_devices, normalize_device_names(lists.input_devices.clone()));
        assert_eq!(lists.output_devices, normalize_device_names(lists.output_devices.clone()));
    }
}
