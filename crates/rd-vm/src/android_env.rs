//! Host values for Android's shared-storage directory constants.
//!
//! `Environment.getExternalStoragePublicDirectory` receives these strings from
//! static fields such as `DIRECTORY_DOWNLOADS`. Keeping the field values in a
//! host hook avoids treating platform constants as absent DEX statics (null).

use crate::engine::Engine;
use crate::err::VmExit;
use crate::framework::ENVIRONMENT;
#[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
#[allow(unused_imports)] // A single explicit alloc prelude for inherited modules.
use crate::port_prelude::*;
use crate::value::Value;

fn string(vm: &mut Engine, text: &str) -> Result<Value, VmExit> {
    Ok(Value::Obj(crate::intrinsics::alloc_string(
        vm,
        text.to_owned(),
    )?))
}

pub(crate) fn host_static(
    vm: &mut Engine,
    owner: &str,
    field: &str,
) -> Result<Option<Value>, VmExit> {
    if owner != ENVIRONMENT {
        return Ok(None);
    }

    let directory = match field {
        "DIRECTORY_MUSIC" => "Music",
        "DIRECTORY_PODCASTS" => "Podcasts",
        "DIRECTORY_RINGTONES" => "Ringtones",
        "DIRECTORY_ALARMS" => "Alarms",
        "DIRECTORY_NOTIFICATIONS" => "Notifications",
        "DIRECTORY_PICTURES" => "Pictures",
        "DIRECTORY_MOVIES" => "Movies",
        "DIRECTORY_DOWNLOADS" => "Download",
        "DIRECTORY_DCIM" => "DCIM",
        "DIRECTORY_DOCUMENTS" => "Documents",
        "DIRECTORY_SCREENSHOTS" => "Pictures/Screenshots",
        "DIRECTORY_RECORDINGS" => "Recordings",
        "DIRECTORY_AUDIOBOOKS" => "Audiobooks",
        _ => return Ok(None),
    };
    Ok(Some(string(vm, directory)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::VmConfig;
    #[cfg(all(not(feature = "std"), feature = "upstream-runtime"))]
    #[allow(unused_imports)]
    use crate::port_prelude::*;

    #[test]
    fn environment_directory_constants_match_android_names() {
        let mut vm = Engine::new(Vec::new(), VmConfig::default());
        for (field, expected) in [
            ("DIRECTORY_DOWNLOADS", "Download"),
            ("DIRECTORY_DOCUMENTS", "Documents"),
            ("DIRECTORY_DCIM", "DCIM"),
            ("DIRECTORY_SCREENSHOTS", "Pictures/Screenshots"),
        ] {
            let Some(Value::Obj(value)) = host_static(&mut vm, ENVIRONMENT, field).unwrap() else {
                panic!("{field} should be a platform String constant");
            };
            assert_eq!(vm.heap.as_str(value).unwrap(), expected);
        }
        assert!(host_static(&mut vm, ENVIRONMENT, "NO_SUCH_DIRECTORY")
            .unwrap()
            .is_none());
    }
}
