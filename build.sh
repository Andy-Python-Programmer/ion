SPATH=$(dirname $(readlink -f "$0"))
ION_BUILD=$SPATH/build

set -x -e

cargo build --target $1

if [ -d $ION_BUILD ]; then
    sudo rm -rf $ION_BUILD
fi

mkdir $ION_BUILD

dd if=/dev/zero bs=1M count=0 seek=64 of=$ION_BUILD/ion.hdd

parted -s $ION_BUILD/ion.hdd mklabel gpt
parted -s $ION_BUILD/ion.hdd mkpart primary 2048s 100%

mkdir $ION_BUILD/mnt

sudo losetup -Pf --show $ION_BUILD/ion.hdd > loopback_dev
sudo mkfs.fat -F 32 `cat loopback_dev`p1
sudo mount `cat loopback_dev`p1 build/mnt

sudo mkdir -p $ION_BUILD/mnt/EFI/BOOT
sudo cp $SPATH/target/x86_64-unknown-uefi/debug/ion.efi $ION_BUILD/mnt/EFI/BOOT/BOOTX64.EFI
sudo cp $SPATH/ion.cfg $ION_BUILD/mnt/ion.cfg

sync

sudo umount $ION_BUILD/mnt
sudo losetup -d `cat loopback_dev`

rm -rf $ION_BUILD/mnt/ loopback_dev
