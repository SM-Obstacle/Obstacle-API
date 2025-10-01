# GraphQL Records Pagination

## Overview

The API now supports cursor-based pagination for loading records through the GraphQL endpoint. This allows you to load more records efficiently, handling cases where records might be inserted between requests.

## New Fields

Three new `recordsConnection` fields have been added:

1. **QueryRoot.recordsConnection** - For fetching all records with pagination
2. **Player.recordsConnection** - For fetching a specific player's records with pagination
3. **Map.recordsConnection** - For fetching records for a specific map with pagination

## Backward Compatibility

The old `records` fields remain unchanged and will continue to work as before:
- `QueryRoot.records`
- `Player.records`
- `Map.records`

These old fields return a simple list limited to 100 records.

## Usage Examples

### Basic Forward Pagination

Fetch the first 50 records (default):

```graphql
query {
  recordsConnection {
    edges {
      node {
        id
        rank
        time
        player {
          login
          name
        }
      }
      cursor
    }
    pageInfo {
      hasNextPage
      hasPreviousPage
      endCursor
    }
  }
}
```

### Fetch Next Page

Using the `endCursor` from the previous response:

```graphql
query {
  recordsConnection(first: 50, after: "cmVjb3JkOjE3MDkyMzQ1Njc4OTA=") {
    edges {
      node {
        id
        rank
        time
      }
      cursor
    }
    pageInfo {
      hasNextPage
      endCursor
    }
  }
}
```

### Player Records with Pagination

```graphql
query {
  player(login: "player_login") {
    recordsConnection(first: 25) {
      edges {
        node {
          id
          rank
          time
          map {
            name
          }
        }
        cursor
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
```

### Map Records with Pagination

```graphql
query {
  map(gameId: "map_uid") {
    recordsConnection(first: 30, rankSortBy: SORT) {
      edges {
        node {
          id
          rank
          time
          player {
            login
            name
          }
        }
        cursor
      }
      pageInfo {
        hasNextPage
        endCursor
      }
    }
  }
}
```

### Backward Pagination

Fetch the last 20 records:

```graphql
query {
  recordsConnection(last: 20) {
    edges {
      node {
        id
        rank
        time
      }
      cursor
    }
    pageInfo {
      hasPreviousPage
      startCursor
    }
  }
}
```

## Parameters

All `recordsConnection` fields support the following parameters:

- **after** (String): Cursor to fetch records after (for forward pagination)
- **before** (String): Cursor to fetch records before (for backward pagination)
- **first** (Int): Number of records to fetch (1-100, default: 50)
- **last** (Int): Number of records to fetch from the end (for backward pagination, 1-100)
- **dateSortBy** (SortState): Sort by date (SORT for descending, REVERSE for ascending)
- **rankSortBy** (SortState): Sort by rank (Map records only)

## Constraints

- Cannot use both `first` and `last` together
- Cannot use both `after` and `before` together
- `first` and `last` must be between 1 and 100
- Default limit is 50 records if neither `first` nor `last` is specified

## Response Structure

### RecordsConnection
- **edges**: Array of RankedRecordEdge
- **pageInfo**: PageInfo object

### RankedRecordEdge
- **node**: The RankedRecord object
- **cursor**: Opaque cursor string for this record

### PageInfo
- **hasNextPage**: Whether there are more records after this page
- **hasPreviousPage**: Whether there are records before this page
- **startCursor**: Cursor of the first record in this page
- **endCursor**: Cursor of the last record in this page

## Cursor Format

Cursors are opaque base64-encoded strings that encode the record date timestamp. They should be treated as opaque values and passed as-is to subsequent requests.

## Notes

- Cursors are based on record dates to handle records being inserted between requests
- The pagination is most efficient when used with date-based sorting
- For map records with rank-based sorting, the implementation uses time-based ordering which correlates with rank
