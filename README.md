# Agon

## Run

```
cp .env.example .env
docker compose up -d
make run
```

## Test

```
make test
```

## TODO

- Random team assignment
    - Change the API so players are taking in a separate list and contain a team id/index, then we can support random 
- Join/share games
    - UI for groups looking for games
    - UI for ringers looking for games
    - UI for games looking for ringers
- Limit max number of accepted invites per team
- Add API validation for number of teams e.g. ensure always 1 or 2
- Add an list of notiifaction - e.g. you have a notifaction
- Check for conflicts with other games
- Calendar/schedule view
- ICS
- Recurring
    - Store game metadata separtely (for all games) so updating a recurring (excluding dates/times) is a single record
- Location
    - Allow users to type to find location
    - Show location on the map
    - Add in Agon venues which we book for them

- Notifications
- Game announcements

## Database schema

### User

- id
- name
- email

### Group

- id
- name

### Group invite

- group_id
- user_id

### Group membership

- group_id

### Game series

- id
- name
- default team/group invites
- default location
- default duration

- start date
- end date -> optional
- schedule -> optional if recurring

### Game instance

- game_series_id
- start time
- overrides for defaults from series

### Venue

- id
- long, lat
- 

## Opertaions

- Create game (optionally recurring)
    - This creates a game series and one/many game instances
    - It can include a deafult venue
- Upgate game
- Get game
- List user games
    - Filter between given date range

- Submit game result

- Sign up/in
- Update profile
