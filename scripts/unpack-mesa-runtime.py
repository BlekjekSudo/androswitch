"""Safely unpack the hash-verified optional Mesa runtime component."""
import argparse
from pathlib import Path
import tarfile
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--archive',type=Path,required=True)
p.add_argument('--destination',type=Path,required=True)
a=p.parse_args()
with tarfile.open(a.archive) as archive:
    members=archive.getmembers()
    assert sum(m.size for m in members)<512*1024*1024
    for m in members:
        assert (a.destination/m.name).resolve().is_relative_to(a.destination.resolve())
        assert not m.issym() and not m.islnk()
    archive.extractall(a.destination,filter='data')
