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
- Location
    - Allow users to type to find location
    - Show location on the map
    - Add in Agon venues which we book for them

- Notifications
- Game announcements
