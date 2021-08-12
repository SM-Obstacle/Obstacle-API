from dataclasses import dataclass
import mysql.connector as database
import math

@dataclass
class MapStat:
    records_count: float = 0.0
    min_record: float = 0.0
    average_record: float = 0.0
    median_record: float = 0.0
    max_record: float = 0.0
    score: float = 0.0

@dataclass
class PlayerStat:
    score: float = 0.0

def record_time_sec(record):
    return record[3] / 1000.0

def compute_score(r, rn, t, average):
    score = math.log10(1000 * (rn * rn)) + math.log10((average-t)**2 + 1)
    score = score * math.log10((rn/r) + 1)**3
    return score

# ==============================================================================================

connection = database.connect(
        user="root",
        password="root",
        host="localhost",
        database="obstacle_records")

cursor = connection.cursor(prepared=True)
cursor.execute("SELECT * FROM players;")

players = cursor.fetchall()
player_ids = [ player[0] for player in players ]
players = dict(zip(player_ids, players))

cursor.execute("SELECT * FROM maps;")
maps = cursor.fetchall()
map_ids = [ map[0] for map in maps ]
maps = dict(zip(map_ids, maps))

map_stats = dict()
player_stats = dict()

for map_id, map in maps.items():

    cursor.execute("""SELECT * from records WHERE map_id = %s ORDER BY time""", (map_id,))
    map_records = cursor.fetchall()

    if (len(map_records) == 0):
        continue

    stats = MapStat()
    stats.min_record = record_time_sec(map_records[0])
    stats.max_record = record_time_sec(map_records[0])
    stats.records_count = len(map_records)
    for record in map_records:
        stats.min_record = min(stats.min_record, record_time_sec(record))
        stats.max_record = max(stats.max_record, record_time_sec(record))
        stats.average_record += record_time_sec(record)
    stats.average_record = stats.average_record / stats.records_count
    stats.median_record = record_time_sec(map_records[len(map_records)//2])

    for i_record in range(len(map_records)):
        record = map_records[i_record]

        r = i_record + 1
        rn = stats.records_count
        t = max(record_time_sec(record), stats.average_record)
        score = compute_score(r, rn, t, stats.average_record)

        stats.score += score

        if record[1] not in player_stats:
            player_stats[record[1]] = PlayerStat()
        player_stats[record[1]].score += score

    map_stats[map_id] = stats


# Sort player and map stats
def StatTupleKey(elem):
    return elem[1].score
sorted_player_stats = list(player_stats.items())
sorted_player_stats.sort(key=StatTupleKey, reverse=True)

sorted_map_stats = list(map_stats.items())
sorted_map_stats.sort(key=StatTupleKey, reverse=True)

# Write the stats to csv files
with open('python_players_ladder.csv', 'w', encoding='utf-8') as f:
    f.write('id,login,name,score\n')

    for player_id, player_stats in sorted_player_stats:
        player = players[player_id]
        f.write(f'{player[0]},{player[1]},{player[2]},{player_stats.score}\n')

with open('python_maps_ladder.csv', 'w', encoding='utf-8') as f:
    f.write('id,name,score,average_score,min_record,max_record,average_record,median_record,records_count\n')

    for map_id, map_stats in sorted_map_stats:
        map = maps[map_id]
        average = map_stats.score / map_stats.records_count
        f.write(f'{map[0]},{map[3]},{map_stats.score},{average},{map_stats.min_record},{map_stats.max_record},{map_stats.average_record},{map_stats.median_record},{map_stats.records_count}\n')
