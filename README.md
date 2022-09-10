# nooku
Discord bot to play a looped hourly song with rainy and snowy variants similar to the soundtrack of Animal Crossing. 

## Overview
The user will be able to provide their own music tracks by placing them into the song folder and following the naming convention listed in the folder's README file.

### Usage
If you clone this repository for use there will be a few things you need to do to get it to work:
- You will need to know how to setup a Discord bot and retreieve the bot private API token and use it as the environmental variable DISCORD_TOKEN.
- Populate the songs folder with 72 song files following the naming conventions listed in the README.txt found in the songs folder.
- Generate an API key with https://openweathermap.org/api and put it into a file named api_key in the project directory.

__Example Folder Layout__

- nooku/
  - src/
    - lib.rs
    - main.rs
    - weather.rs
  - songs/
    - (72 songs files)
    - README.TXT
  - api_key (contains the weather API key)
  - secret.bash (Used to **source** the bot API key as an environment variable which is one method of adding environment variables)
  - README.md

__All of this is subject to change!!!__ 

In future versions numerous settings are planned to be contained within a settings file instead of hardcoded or contained in a file.

#### This bot is incomplete!
This bot is a WIP and will often break or throw errors and crash. If you need a stable bot, this is not the bot for you at the moment. I take no responsibility for any harm caused by the bot. The code is inefficient but I am still actively working to improve it. 
