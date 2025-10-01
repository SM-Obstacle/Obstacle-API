# Schema Comparison: Before and After

## Before (Original Implementation)

### QueryRoot
```graphql
type QueryRoot {
  # ... other fields ...
  records(dateSortBy: SortState): [RankedRecord!]!
}
```

### Player
```graphql
type Player {
  id: ID!
  login: String!
  name: String!
  zonePath: String
  role: PlayerRole!
  records(dateSortBy: SortState): [RankedRecord!]!
}
```

### Map
```graphql
type Map {
  id: ID!
  gameId: String!
  # ... other fields ...
  records(rankSortBy: SortState, dateSortBy: SortState): [RankedRecord!]!
}
```

**Limitations:**
- ❌ Fixed limit of 100 records
- ❌ No pagination support
- ❌ Cannot load more records
- ❌ No cursor-based navigation
- ❌ Difficult to handle concurrent inserts

---

## After (With Pagination Support)

### QueryRoot
```graphql
type QueryRoot {
  # ... other fields ...
  
  # ✅ ORIGINAL - Unchanged for backward compatibility
  records(dateSortBy: SortState): [RankedRecord!]!
  
  # ✨ NEW - With cursor-based pagination
  recordsConnection(
    """Cursor to fetch records after (for forward pagination)"""
    after: String
    """Cursor to fetch records before (for backward pagination)"""
    before: String
    """Number of records to fetch (default: 50, max: 100)"""
    first: Int
    """Number of records to fetch from the end (for backward pagination)"""
    last: Int
    dateSortBy: SortState
  ): RecordsConnection!
}
```

### Player
```graphql
type Player {
  id: ID!
  login: String!
  name: String!
  zonePath: String
  role: PlayerRole!
  
  # ✅ ORIGINAL - Unchanged for backward compatibility
  records(dateSortBy: SortState): [RankedRecord!]!
  
  # ✨ NEW - With cursor-based pagination
  recordsConnection(
    """Cursor to fetch records after (for forward pagination)"""
    after: String
    """Cursor to fetch records before (for backward pagination)"""
    before: String
    """Number of records to fetch (default: 50, max: 100)"""
    first: Int
    """Number of records to fetch from the end (for backward pagination)"""
    last: Int
    dateSortBy: SortState
  ): RecordsConnection!
}
```

### Map
```graphql
type Map {
  id: ID!
  gameId: String!
  # ... other fields ...
  
  # ✅ ORIGINAL - Unchanged for backward compatibility
  records(rankSortBy: SortState, dateSortBy: SortState): [RankedRecord!]!
  
  # ✨ NEW - With cursor-based pagination
  recordsConnection(
    """Cursor to fetch records after (for forward pagination)"""
    after: String
    """Cursor to fetch records before (for backward pagination)"""
    before: String
    """Number of records to fetch (default: 50, max: 100)"""
    first: Int
    """Number of records to fetch from the end (for backward pagination)"""
    last: Int
    rankSortBy: SortState
    dateSortBy: SortState
  ): RecordsConnection!
}
```

### New Types

```graphql
type RecordsConnection {
  edges: [RankedRecordEdge!]!
  pageInfo: PageInfo!
}

type RankedRecordEdge {
  node: RankedRecord!
  cursor: String!
}

type PageInfo {
  hasNextPage: Boolean!
  hasPreviousPage: Boolean!
  startCursor: String
  endCursor: String
}
```

**Benefits:**
- ✅ Configurable limits (1-100 records, default: 50)
- ✅ Full pagination support
- ✅ Can load unlimited records via pagination
- ✅ Cursor-based navigation
- ✅ Handles concurrent inserts gracefully
- ✅ Page info (hasNextPage, hasPreviousPage)
- ✅ Both forward and backward pagination
- ✅ **100% backward compatible** - existing queries work unchanged

---

## Migration Path

### For Existing Clients
No changes required! All existing queries continue to work:

```graphql
# This still works exactly as before
query {
  player(login: "player1") {
    records {
      id
      rank
      time
    }
  }
}
```

### For New Clients
Can now use pagination:

```graphql
# Load first 50 records
query {
  player(login: "player1") {
    recordsConnection(first: 50) {
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
}

# Load next 50 records using cursor
query {
  player(login: "player1") {
    recordsConnection(first: 50, after: "cmVjb3JkOjE3MDky...") {
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
}
```

---

## Summary of Changes

| Aspect | Before | After |
|--------|--------|-------|
| **Fields** | 3 fields (records) | 6 fields (3 records + 3 recordsConnection) |
| **Max Records** | 100 per query | Unlimited via pagination |
| **Default Limit** | 100 | 50 (configurable 1-100) |
| **Pagination** | ❌ None | ✅ Cursor-based |
| **Backward Compat** | N/A | ✅ 100% compatible |
| **Concurrent Inserts** | ❌ May cause issues | ✅ Handled gracefully |
| **Navigation** | ❌ None | ✅ Forward & backward |
| **Page Info** | ❌ None | ✅ Full metadata |
| **Types Added** | 0 | 3 (RecordsConnection, RankedRecordEdge, PageInfo) |
