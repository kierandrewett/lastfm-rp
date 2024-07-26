build:
	cargo build --release

install:
	mkdir -p ~/.config/lastfm-rp/
	@if [ ! -f "~/.config/lastfm-rp/lastfm-rp.conf" ]; then \
		touch ~/.config/lastfm-rp/lastfm-rp.conf; \
	fi
	systemctl --user disable lastfm-rp.service
	systemctl --user stop lastfm-rp.service
	cp ./target/release/lastfm-rp ~/.local/bin/lastfm-rp
	cp ./contrib/lastfm-rp.service ~/.config/systemd/user/
	systemctl --user enable lastfm-rp.service
	systemctl --user start lastfm-rp.service
	systemctl --user restart lastfm-rp.service